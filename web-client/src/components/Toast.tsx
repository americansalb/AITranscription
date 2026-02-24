import { useUIStore } from "../lib/stores";

export function ToastContainer() {
  const toasts = useUIStore((s) => s.toasts);
  const removeToast = useUIStore((s) => s.removeToast);

  if (toasts.length === 0) return null;

  return (
    <div className="toast-container" aria-label="Notifications">
      {toasts.map((toast) => (
        <div
          key={toast.id}
          className={`toast toast-${toast.type}`}
          role="alert"
          aria-live={toast.type === "error" ? "assertive" : "polite"}
          onClick={() => removeToast(toast.id)}
          tabIndex={0}
          onKeyDown={(e) => { if (e.key === "Enter" || e.key === "Escape") removeToast(toast.id); }}
          aria-label={`${toast.type}: ${toast.message}. Press Enter or Escape to dismiss.`}
        >
          {toast.message}
        </div>
      ))}
    </div>
  );
}
