import React, { Suspense, lazy } from "react";
import ReactDOM from "react-dom/client";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import "./index.css";

const queryClient = new QueryClient();
const App = lazy(() => import("./App"));

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <Suspense
        fallback={
          <main className="flex min-h-screen items-center justify-center bg-app-bg px-8 text-foreground">
            <div className="rounded-[28px] bg-card px-6 py-4 shadow-[0_18px_36px_rgba(15,23,42,0.08)] ring-1 ring-border">
              Loading Mouser...
            </div>
          </main>
        }
      >
        <App />
      </Suspense>
    </QueryClientProvider>
  </React.StrictMode>,
);
