#[cfg(target_os = "macos")]
use std::{
    ffi::c_void,
    os::raw::{c_int, c_long},
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender, TryRecvError},
        Arc, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[cfg(target_os = "macos")]
use core_foundation::{
    base::{CFRelease, CFRetain, CFTypeRef, TCFType},
    dictionary::CFMutableDictionary,
    number::CFNumber,
    runloop::{kCFRunLoopDefaultMode, CFRunLoop},
    set::{CFSetGetCount, CFSetGetValues, CFSetRef},
    string::CFString,
};

#[cfg(target_os = "macos")]
use crate::PlatformError;

#[cfg(target_os = "macos")]
type IOOptionBits = u32;
#[cfg(target_os = "macos")]
type IOReturn = i32;
#[cfg(target_os = "macos")]
type IOHIDManagerRef = *mut c_void;
#[cfg(target_os = "macos")]
type IOHIDDeviceRef = *mut c_void;
#[cfg(target_os = "macos")]
type IOHIDElementRef = *const c_void;
#[cfg(target_os = "macos")]
type IOHIDReportType = c_int;
#[cfg(target_os = "macos")]
type IOHIDValueRef = *const c_void;
#[cfg(target_os = "macos")]
type IOHIDDeviceCallback = unsafe extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    device: IOHIDDeviceRef,
);

#[cfg(target_os = "macos")]
const LOGI_VID: u16 = 0x046D;
#[cfg(target_os = "macos")]
const REPORT_TYPE_OUTPUT: IOHIDReportType = 1;

#[cfg(target_os = "macos")]
type IOHIDReportCallback = unsafe extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    report_type: IOHIDReportType,
    report_id: u32,
    report: *mut u8,
    report_length: c_long,
);
#[cfg(target_os = "macos")]
type IOHIDValueCallback = unsafe extern "C" fn(
    context: *mut c_void,
    result: IOReturn,
    sender: *mut c_void,
    value: IOHIDValueRef,
);

#[cfg(target_os = "macos")]
#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOHIDManagerCreate(allocator: *const c_void, options: IOOptionBits) -> IOHIDManagerRef;
    fn IOHIDManagerSetDeviceMatching(manager: IOHIDManagerRef, matching: *const c_void);
    fn IOHIDManagerOpen(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerCopyDevices(manager: IOHIDManagerRef) -> CFSetRef;
    fn IOHIDManagerClose(manager: IOHIDManagerRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDManagerScheduleWithRunLoop(
        manager: IOHIDManagerRef,
        run_loop: *mut c_void,
        run_loop_mode: *const c_void,
    );
    fn IOHIDManagerUnscheduleFromRunLoop(
        manager: IOHIDManagerRef,
        run_loop: *mut c_void,
        run_loop_mode: *const c_void,
    );
    fn IOHIDManagerRegisterDeviceMatchingCallback(
        manager: IOHIDManagerRef,
        callback: IOHIDDeviceCallback,
        context: *mut c_void,
    );
    fn IOHIDManagerRegisterDeviceRemovalCallback(
        manager: IOHIDManagerRef,
        callback: IOHIDDeviceCallback,
        context: *mut c_void,
    );

    fn IOHIDDeviceOpen(device: IOHIDDeviceRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceClose(device: IOHIDDeviceRef, options: IOOptionBits) -> IOReturn;
    fn IOHIDDeviceGetProperty(device: IOHIDDeviceRef, key: *const c_void) -> CFTypeRef;
    fn IOHIDDeviceScheduleWithRunLoop(
        device: IOHIDDeviceRef,
        run_loop: *mut c_void,
        run_loop_mode: *const c_void,
    );
    fn IOHIDDeviceUnscheduleFromRunLoop(
        device: IOHIDDeviceRef,
        run_loop: *mut c_void,
        run_loop_mode: *const c_void,
    );
    fn IOHIDDeviceSetReport(
        device: IOHIDDeviceRef,
        report_type: IOHIDReportType,
        report_id: c_long,
        report: *const u8,
        report_length: c_long,
    ) -> IOReturn;
    fn IOHIDDeviceRegisterInputReportCallback(
        device: IOHIDDeviceRef,
        report: *mut u8,
        report_length: c_long,
        callback: IOHIDReportCallback,
        context: *mut c_void,
    );
    fn IOHIDDeviceRegisterInputValueCallback(
        device: IOHIDDeviceRef,
        callback: IOHIDValueCallback,
        context: *mut c_void,
    );
    fn IOHIDValueGetElement(value: IOHIDValueRef) -> IOHIDElementRef;
    fn IOHIDValueGetIntegerValue(value: IOHIDValueRef) -> c_long;
    fn IOHIDElementGetUsagePage(element: IOHIDElementRef) -> u32;
    fn IOHIDElementGetUsage(element: IOHIDElementRef) -> u32;
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub struct MacOsIoKitInfo {
    pub product_id: u16,
    pub usage_page: u32,
    pub usage: u32,
    pub transport: Option<String>,
    pub product_string: Option<String>,
    pub serial_number: Option<String>,
    pub location_id: Option<u32>,
}

#[cfg(target_os = "macos")]
#[derive(Debug, Clone)]
pub struct MacOsInputValueEvent {
    pub usage_page: u32,
    pub usage: u32,
    pub value: i64,
    pub observed_at: Instant,
}

#[cfg(target_os = "macos")]
pub fn enumerate_iokit_infos() -> Result<Vec<MacOsIoKitInfo>, PlatformError> {
    let manager = unsafe { IOHIDManagerCreate(ptr::null(), 0) };
    if manager.is_null() {
        return Err(PlatformError::Message(
            "IOHIDManagerCreate failed".to_string(),
        ));
    }

    let matching = vendor_matching_dictionary(LOGI_VID);
    let devices = unsafe {
        IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef() as *const c_void);
        let status = IOHIDManagerOpen(manager, 0);
        if status != 0 {
            CFRelease(manager as CFTypeRef);
            return Err(PlatformError::Message(format!(
                "IOHIDManagerOpen failed: 0x{status:08X}"
            )));
        }
        IOHIDManagerCopyDevices(manager)
    };

    let mut infos = Vec::new();
    let mut seen = std::collections::BTreeSet::new();

    if !devices.is_null() {
        for device_ref in copy_set_values(devices) {
            let product_id = match get_number_property(device_ref as IOHIDDeviceRef, "ProductID") {
                Some(value) if value > 0 => value as u16,
                _ => continue,
            };
            let usage_page = get_number_property(device_ref as IOHIDDeviceRef, "PrimaryUsagePage")
                .unwrap_or(0) as u32;
            let usage = get_number_property(device_ref as IOHIDDeviceRef, "PrimaryUsage")
                .unwrap_or(0) as u32;
            let transport = get_string_property(device_ref as IOHIDDeviceRef, "Transport");
            let product_string = get_string_property(device_ref as IOHIDDeviceRef, "Product");
            let serial_number = get_string_property(device_ref as IOHIDDeviceRef, "SerialNumber");
            let location_id = get_number_property(device_ref as IOHIDDeviceRef, "LocationID")
                .map(|value| value as u32);
            let dedupe_key = (
                product_id,
                usage_page,
                usage,
                transport.clone().unwrap_or_default(),
                product_string.clone().unwrap_or_default(),
                serial_number.clone().unwrap_or_default(),
                location_id.unwrap_or_default(),
            );
            if seen.insert(dedupe_key) {
                infos.push(MacOsIoKitInfo {
                    product_id,
                    usage_page,
                    usage,
                    transport,
                    product_string,
                    serial_number,
                    location_id,
                });
            }
        }
        unsafe { CFRelease(devices as CFTypeRef) };
    }

    unsafe { CFRelease(manager as CFTypeRef) };

    Ok(infos)
}

#[cfg(target_os = "macos")]
struct DeviceMonitorContext {
    notify: Box<dyn Fn() + Send + Sync>,
    stopped: Arc<AtomicBool>,
}

#[cfg(target_os = "macos")]
pub struct MacOsDeviceMonitor {
    run_loop: Arc<Mutex<Option<CFRunLoop>>>,
    stopped: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

#[cfg(target_os = "macos")]
impl MacOsDeviceMonitor {
    pub fn new<F>(notify: F) -> Result<Self, PlatformError>
    where
        F: Fn() + Send + Sync + 'static,
    {
        let run_loop = Arc::new(Mutex::new(None));
        let stopped = Arc::new(AtomicBool::new(false));
        let (startup_tx, startup_rx) = mpsc::channel();

        let worker_run_loop = Arc::clone(&run_loop);
        let worker_stopped = Arc::clone(&stopped);
        let worker = thread::Builder::new()
            .name("mouser-macos-device-monitor".to_string())
            .spawn(move || {
                let manager = unsafe { IOHIDManagerCreate(ptr::null(), 0) };
                if manager.is_null() {
                    let _ = startup_tx.send(Err(PlatformError::Message(
                        "IOHIDManagerCreate failed".to_string(),
                    )));
                    return;
                }

                let matching = vendor_matching_dictionary(LOGI_VID);
                let context = Box::into_raw(Box::new(DeviceMonitorContext {
                    notify: Box::new(notify),
                    stopped: Arc::clone(&worker_stopped),
                }));

                unsafe {
                    IOHIDManagerSetDeviceMatching(
                        manager,
                        matching.as_concrete_TypeRef() as *const c_void,
                    );
                    IOHIDManagerRegisterDeviceMatchingCallback(
                        manager,
                        device_monitor_callback,
                        context as *mut c_void,
                    );
                    IOHIDManagerRegisterDeviceRemovalCallback(
                        manager,
                        device_monitor_callback,
                        context as *mut c_void,
                    );
                }

                let current_run_loop = CFRunLoop::get_current();
                *worker_run_loop.lock().unwrap() = Some(current_run_loop.clone());

                unsafe {
                    IOHIDManagerScheduleWithRunLoop(
                        manager,
                        current_run_loop.as_concrete_TypeRef() as *mut c_void,
                        kCFRunLoopDefaultMode as *const c_void,
                    );
                }

                let open_status = unsafe { IOHIDManagerOpen(manager, 0) };
                if open_status != 0 {
                    unsafe {
                        IOHIDManagerUnscheduleFromRunLoop(
                            manager,
                            current_run_loop.as_concrete_TypeRef() as *mut c_void,
                            kCFRunLoopDefaultMode as *const c_void,
                        );
                        CFRelease(manager as CFTypeRef);
                        drop(Box::from_raw(context));
                    }
                    *worker_run_loop.lock().unwrap() = None;
                    let _ = startup_tx.send(Err(PlatformError::Message(format!(
                        "IOHIDManagerOpen failed: 0x{open_status:08X}"
                    ))));
                    return;
                }

                let _ = startup_tx.send(Ok(()));
                CFRunLoop::run_current();

                unsafe {
                    IOHIDManagerUnscheduleFromRunLoop(
                        manager,
                        current_run_loop.as_concrete_TypeRef() as *mut c_void,
                        kCFRunLoopDefaultMode as *const c_void,
                    );
                    let _ = IOHIDManagerClose(manager, 0);
                    CFRelease(manager as CFTypeRef);
                    drop(Box::from_raw(context));
                }
                *worker_run_loop.lock().unwrap() = None;
            })
            .map_err(|error| PlatformError::Message(error.to_string()))?;

        match startup_rx.recv_timeout(Duration::from_millis(800)) {
            Ok(Ok(())) => Ok(Self {
                run_loop,
                stopped,
                worker: Some(worker),
            }),
            Ok(Err(error)) => {
                let _ = worker.join();
                Err(error)
            }
            Err(_) => Err(PlatformError::Message(
                "Timed out while starting the macOS HID device monitor".to_string(),
            )),
        }
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacOsDeviceMonitor {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::SeqCst);
        if let Some(run_loop) = self.run_loop.lock().unwrap().clone() {
            run_loop.stop();
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn device_monitor_callback(
    context: *mut c_void,
    _result: IOReturn,
    _sender: *mut c_void,
    _device: IOHIDDeviceRef,
) {
    if context.is_null() {
        return;
    }

    let context = unsafe { &*(context as *const DeviceMonitorContext) };
    if context.stopped.load(Ordering::SeqCst) {
        return;
    }

    (context.notify)();
}

#[cfg(target_os = "macos")]
pub struct MacOsNativeHidDevice {
    manager: IOHIDManagerRef,
    _matching: CFMutableDictionary<*const c_void, *const c_void>,
    device: IOHIDDeviceRef,
    run_loop: CFRunLoop,
    _report_buffer: Box<[u8; 64]>,
    report_sender: *mut Sender<Vec<u8>>,
    report_receiver: Receiver<Vec<u8>>,
    value_sender: *mut Sender<MacOsInputValueEvent>,
    value_receiver: Receiver<MacOsInputValueEvent>,
}

#[cfg(target_os = "macos")]
impl MacOsNativeHidDevice {
    pub fn open(info: &MacOsIoKitInfo) -> Result<Self, PlatformError> {
        let matching = device_matching_dictionary(info);
        let manager = unsafe { IOHIDManagerCreate(ptr::null(), 0) };
        if manager.is_null() {
            return Err(PlatformError::Message(
                "IOHIDManagerCreate failed".to_string(),
            ));
        }

        let device = unsafe {
            IOHIDManagerSetDeviceMatching(manager, matching.as_concrete_TypeRef() as *const c_void);
            let status = IOHIDManagerOpen(manager, 0);
            if status != 0 {
                CFRelease(manager as CFTypeRef);
                return Err(PlatformError::Message(format!(
                    "IOHIDManagerOpen failed: 0x{status:08X}"
                )));
            }

            let devices = IOHIDManagerCopyDevices(manager);
            if devices.is_null() {
                CFRelease(manager as CFTypeRef);
                return Err(PlatformError::Message(describe_match_failure(info)));
            }

            let retained = copy_set_values(devices)
                .into_iter()
                .next()
                .map(|value| CFRetain(value) as IOHIDDeviceRef)
                .ok_or_else(|| PlatformError::Message(describe_match_failure(info)))?;
            CFRelease(devices as CFTypeRef);
            retained
        };

        let open_status = unsafe { IOHIDDeviceOpen(device, 0) };
        if open_status != 0 {
            unsafe {
                CFRelease(device as CFTypeRef);
                CFRelease(manager as CFTypeRef);
            }
            return Err(PlatformError::Message(format!(
                "IOHIDDeviceOpen failed: 0x{open_status:08X}"
            )));
        }

        let run_loop = CFRunLoop::get_current();
        let mut report_buffer = Box::new([0u8; 64]);
        let (report_tx, report_rx) = mpsc::channel();
        let report_sender = Box::into_raw(Box::new(report_tx));
        let (value_tx, value_rx) = mpsc::channel();
        let value_sender = Box::into_raw(Box::new(value_tx));

        unsafe {
            IOHIDDeviceScheduleWithRunLoop(
                device,
                run_loop.as_concrete_TypeRef() as *mut c_void,
                kCFRunLoopDefaultMode as *const c_void,
            );
            IOHIDDeviceRegisterInputReportCallback(
                device,
                report_buffer.as_mut_ptr(),
                report_buffer.len() as c_long,
                input_report_callback,
                report_sender as *mut c_void,
            );
            IOHIDDeviceRegisterInputValueCallback(
                device,
                input_value_callback,
                value_sender as *mut c_void,
            );
        }

        Ok(Self {
            manager,
            _matching: matching,
            device,
            run_loop,
            _report_buffer: report_buffer,
            report_sender,
            report_receiver: report_rx,
            value_sender,
            value_receiver: value_rx,
        })
    }

    pub fn write_report(&self, packet: &[u8]) -> Result<(), PlatformError> {
        let status = unsafe {
            IOHIDDeviceSetReport(
                self.device,
                REPORT_TYPE_OUTPUT,
                packet.first().copied().unwrap_or_default() as c_long,
                packet.as_ptr(),
                packet.len() as c_long,
            )
        };
        if status != 0 {
            return Err(PlatformError::Message(format!(
                "IOHIDDeviceSetReport failed: 0x{status:08X}"
            )));
        }
        Ok(())
    }

    pub fn read_timeout(&self, timeout_ms: i32) -> Result<Vec<u8>, PlatformError> {
        match self.report_receiver.try_recv() {
            Ok(packet) => return Ok(packet),
            Err(TryRecvError::Disconnected) => {
                return Err(PlatformError::Message(
                    "native HID report channel closed".to_string(),
                ))
            }
            Err(TryRecvError::Empty) => {}
        }

        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms.max(0) as u64);

        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let slice = remaining.min(Duration::from_millis(50));
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, slice, true);

            match self.report_receiver.try_recv() {
                Ok(packet) => return Ok(packet),
                Err(TryRecvError::Disconnected) => {
                    return Err(PlatformError::Message(
                        "native HID report channel closed".to_string(),
                    ))
                }
                Err(TryRecvError::Empty) => continue,
            }
        }

        Ok(Vec::new())
    }

    pub fn read_value_timeout(
        &self,
        timeout_ms: i32,
    ) -> Result<Option<MacOsInputValueEvent>, PlatformError> {
        match self.value_receiver.try_recv() {
            Ok(event) => return Ok(Some(event)),
            Err(TryRecvError::Disconnected) => {
                return Err(PlatformError::Message(
                    "native HID input-value channel closed".to_string(),
                ))
            }
            Err(TryRecvError::Empty) => {}
        }

        let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms.max(0) as u64);

        while std::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let slice = remaining.min(Duration::from_millis(50));
            CFRunLoop::run_in_mode(unsafe { kCFRunLoopDefaultMode }, slice, true);

            match self.value_receiver.try_recv() {
                Ok(event) => return Ok(Some(event)),
                Err(TryRecvError::Disconnected) => {
                    return Err(PlatformError::Message(
                        "native HID input-value channel closed".to_string(),
                    ))
                }
                Err(TryRecvError::Empty) => continue,
            }
        }

        Ok(None)
    }
}

#[cfg(target_os = "macos")]
impl Drop for MacOsNativeHidDevice {
    fn drop(&mut self) {
        unsafe {
            IOHIDDeviceUnscheduleFromRunLoop(
                self.device,
                self.run_loop.as_concrete_TypeRef() as *mut c_void,
                kCFRunLoopDefaultMode as *const c_void,
            );
            IOHIDDeviceClose(self.device, 0);
            CFRelease(self.device as CFTypeRef);
            CFRelease(self.manager as CFTypeRef);
            drop(Box::from_raw(self.report_sender));
            drop(Box::from_raw(self.value_sender));
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn input_report_callback(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    _report_type: IOHIDReportType,
    _report_id: u32,
    report: *mut u8,
    report_length: c_long,
) {
    if result != 0 || context.is_null() || report.is_null() || report_length <= 0 {
        return;
    }

    let sender = &*(context as *mut Sender<Vec<u8>>);
    let packet = std::slice::from_raw_parts(report, report_length as usize).to_vec();
    let _ = sender.send(packet);
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn input_value_callback(
    context: *mut c_void,
    result: IOReturn,
    _sender: *mut c_void,
    value: IOHIDValueRef,
) {
    if result != 0 || context.is_null() || value.is_null() {
        return;
    }

    let element = IOHIDValueGetElement(value);
    if element.is_null() {
        return;
    }

    let sender = &*(context as *mut Sender<MacOsInputValueEvent>);
    let _ = sender.send(MacOsInputValueEvent {
        usage_page: IOHIDElementGetUsagePage(element),
        usage: IOHIDElementGetUsage(element),
        value: IOHIDValueGetIntegerValue(value) as i64,
        observed_at: Instant::now(),
    });
}

#[cfg(target_os = "macos")]
fn vendor_matching_dictionary(vendor_id: u16) -> CFMutableDictionary<*const c_void, *const c_void> {
    let mut dictionary = CFMutableDictionary::new();
    let key = CFString::new("VendorID");
    let value = CFNumber::from(vendor_id as i32);
    dictionary.set(
        key.as_concrete_TypeRef() as *const c_void,
        value.as_concrete_TypeRef() as *const c_void,
    );
    dictionary
}

#[cfg(target_os = "macos")]
fn device_matching_dictionary(
    info: &MacOsIoKitInfo,
) -> CFMutableDictionary<*const c_void, *const c_void> {
    let mut dictionary = vendor_matching_dictionary(LOGI_VID);

    set_number_entry(&mut dictionary, "ProductID", info.product_id as i32);
    if info.usage_page > 0 {
        set_number_entry(&mut dictionary, "PrimaryUsagePage", info.usage_page as i32);
    }
    if info.usage > 0 {
        set_number_entry(&mut dictionary, "PrimaryUsage", info.usage as i32);
    }
    if let Some(transport) = info.transport.as_deref() {
        set_string_entry(&mut dictionary, "Transport", transport);
    }

    dictionary
}

#[cfg(target_os = "macos")]
fn set_number_entry(
    dictionary: &mut CFMutableDictionary<*const c_void, *const c_void>,
    key: &str,
    value: i32,
) {
    let key = CFString::new(key);
    let value = CFNumber::from(value);
    dictionary.set(
        key.as_concrete_TypeRef() as *const c_void,
        value.as_concrete_TypeRef() as *const c_void,
    );
}

#[cfg(target_os = "macos")]
fn set_string_entry(
    dictionary: &mut CFMutableDictionary<*const c_void, *const c_void>,
    key: &str,
    value: &str,
) {
    let key = CFString::new(key);
    let value = CFString::new(value);
    dictionary.set(
        key.as_concrete_TypeRef() as *const c_void,
        value.as_concrete_TypeRef() as *const c_void,
    );
}

#[cfg(target_os = "macos")]
fn copy_set_values(set_ref: CFSetRef) -> Vec<*const c_void> {
    let count = unsafe { CFSetGetCount(set_ref) };
    if count <= 0 {
        return Vec::new();
    }

    let mut values = vec![ptr::null(); count as usize];
    unsafe { CFSetGetValues(set_ref, values.as_mut_ptr()) };
    values
}

#[cfg(target_os = "macos")]
fn get_number_property(device: IOHIDDeviceRef, name: &str) -> Option<i32> {
    let key = CFString::new(name);
    let value_ref = unsafe { IOHIDDeviceGetProperty(device, key.as_concrete_TypeRef() as _) };
    if value_ref.is_null() {
        return None;
    }

    let number = unsafe { CFNumber::wrap_under_get_rule(value_ref as _) };
    number.to_i32()
}

#[cfg(target_os = "macos")]
fn get_string_property(device: IOHIDDeviceRef, name: &str) -> Option<String> {
    let key = CFString::new(name);
    let value_ref = unsafe { IOHIDDeviceGetProperty(device, key.as_concrete_TypeRef() as _) };
    if value_ref.is_null() {
        return None;
    }

    let string = unsafe { CFString::wrap_under_get_rule(value_ref as _) };
    Some(string.to_string())
}

#[cfg(target_os = "macos")]
fn describe_match_failure(info: &MacOsIoKitInfo) -> String {
    let mut parts = vec![format!("PID 0x{:04X}", info.product_id)];
    if info.usage_page > 0 {
        parts.push(format!("UP 0x{:04X}", info.usage_page));
    }
    if info.usage > 0 {
        parts.push(format!("usage 0x{:04X}", info.usage));
    }
    if let Some(transport) = info.transport.as_deref() {
        parts.push(format!("transport \"{transport}\""));
    }
    format!("No IOHIDDevice for {}", parts.join(" "))
}
