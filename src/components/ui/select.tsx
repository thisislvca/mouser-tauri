import * as React from "react";
import { Select as BaseSelect } from "@base-ui/react/select";
import { CaretDown, Check } from "@phosphor-icons/react";
import { cn } from "../../lib/utils";

export type SelectOption = {
  label: string;
  value: string;
  group?: string;
};

type SelectProps = {
  ariaLabel: string;
  className?: string;
  disabled?: boolean;
  options: SelectOption[];
  placeholder?: string;
  value: string;
  onValueChange: (value: string) => void;
};

function Select({ ariaLabel, className, disabled, onValueChange, options, placeholder, value }: SelectProps) {
  const labelLookup = React.useMemo(
    () => new Map(options.map((option) => [option.value, option.label])),
    [options],
  );
  const groupedOptions = React.useMemo(() => {
    const groups = new Map<string | undefined, SelectOption[]>();
    options.forEach((option) => {
      const next = groups.get(option.group) ?? [];
      next.push(option);
      groups.set(option.group, next);
    });
    return [...groups.entries()];
  }, [options]);

  return (
    <BaseSelect.Root
      disabled={disabled}
      modal={false}
      onValueChange={(nextValue) => onValueChange((nextValue as string | null) ?? "")}
      value={value || null}
    >
      <BaseSelect.Trigger
        aria-label={ariaLabel}
        className={cn(
          "flex h-11 w-full items-center justify-between rounded-2xl border border-[var(--input)] bg-white px-4 py-2.5 text-left text-sm text-[var(--foreground)] shadow-[inset_0_1px_0_rgba(255,255,255,0.8)] outline-none transition",
          "focus-visible:border-[var(--accent)] focus-visible:ring-4 focus-visible:ring-[color-mix(in_srgb,var(--accent)_14%,transparent)]",
          "data-[popup-open]:border-[var(--accent)]",
          disabled && "cursor-not-allowed opacity-50",
          className,
        )}
      >
        <BaseSelect.Value placeholder={placeholder}>
          {(selectedValue) =>
            selectedValue ? labelLookup.get(selectedValue as string) ?? selectedValue : placeholder
          }
        </BaseSelect.Value>
        <BaseSelect.Icon className="text-[var(--muted-foreground)]">
          <CaretDown className="h-4 w-4" />
        </BaseSelect.Icon>
      </BaseSelect.Trigger>

      <BaseSelect.Portal>
        <BaseSelect.Positioner className="z-50 outline-none" sideOffset={8}>
          <BaseSelect.Popup className="min-w-[240px] rounded-3xl border border-[var(--border)] bg-[var(--card)] p-1.5 shadow-[0_24px_60px_rgba(15,23,42,0.14)] outline-none">
            <BaseSelect.List className="max-h-[320px] overflow-y-auto">
              {groupedOptions.map(([group, groupItems]) =>
                group ? (
                  <BaseSelect.Group className="px-1 py-1" key={group}>
                    <BaseSelect.GroupLabel className="px-3 py-2 text-[11px] font-semibold uppercase tracking-[0.24em] text-[var(--muted-foreground)]">
                      {group}
                    </BaseSelect.GroupLabel>
                    {groupItems.map((option) => (
                      <BaseSelect.Item
                        className={cn(
                          "flex cursor-default items-center justify-between rounded-2xl px-3 py-2.5 text-sm text-[var(--foreground)] outline-none transition",
                          "data-[highlighted]:bg-[var(--muted)]",
                        )}
                        key={option.value}
                        value={option.value}
                      >
                        <BaseSelect.ItemText>{option.label}</BaseSelect.ItemText>
                        <BaseSelect.ItemIndicator className="text-[var(--accent)]">
                          <Check className="h-4 w-4" />
                        </BaseSelect.ItemIndicator>
                      </BaseSelect.Item>
                    ))}
                  </BaseSelect.Group>
                ) : (
                  <React.Fragment key="ungrouped">
                    {groupItems.map((option) => (
                      <BaseSelect.Item
                        className={cn(
                          "flex cursor-default items-center justify-between rounded-2xl px-3 py-2.5 text-sm text-[var(--foreground)] outline-none transition",
                          "data-[highlighted]:bg-[var(--muted)]",
                        )}
                        key={option.value}
                        value={option.value}
                      >
                        <BaseSelect.ItemText>{option.label}</BaseSelect.ItemText>
                        <BaseSelect.ItemIndicator className="text-[var(--accent)]">
                          <Check className="h-4 w-4" />
                        </BaseSelect.ItemIndicator>
                      </BaseSelect.Item>
                    ))}
                  </React.Fragment>
                ),
              )}
            </BaseSelect.List>
          </BaseSelect.Popup>
        </BaseSelect.Positioner>
      </BaseSelect.Portal>
    </BaseSelect.Root>
  );
}

export { Select };
