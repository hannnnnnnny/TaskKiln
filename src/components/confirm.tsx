import { create } from "zustand";
import { Button, Modal } from "./ui";
import { useApp } from "../stores/app";

interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
}

interface ConfirmStore {
  pending: (ConfirmOptions & { resolve: (ok: boolean) => void }) | null;
  ask: (opts: ConfirmOptions) => Promise<boolean>;
  close: (ok: boolean) => void;
}

const useConfirmStore = create<ConfirmStore>((set, get) => ({
  pending: null,
  ask: (opts) =>
    new Promise<boolean>((resolve) => {
      get().pending?.resolve(false);
      set({ pending: { ...opts, resolve } });
    }),
  close: (ok) => {
    get().pending?.resolve(ok);
    set({ pending: null });
  },
}));

/** Ask the user to confirm a destructive or consequential action. */
export const confirmAction = (opts: ConfirmOptions) => useConfirmStore.getState().ask(opts);

export function ConfirmHost() {
  const { pending, close } = useConfirmStore();
  return (
    <Modal
      open={pending !== null}
      title={pending?.title ?? ""}
      onClose={() => close(false)}
      width="max-w-md"
      footer={
        <>
          <Button variant="ghost" onClick={() => close(false)}>
            Cancel
          </Button>
          <Button variant={pending?.danger ? "danger" : "primary"} onClick={() => close(true)}>
            {pending?.confirmLabel ?? "Confirm"}
          </Button>
        </>
      }
    >
      <p className="whitespace-pre-line text-[13px] text-fg/85">{pending?.message}</p>
    </Modal>
  );
}

export function Toasts() {
  const { toasts, dismiss } = useApp();
  if (toasts.length === 0) return null;
  return (
    <div className="pointer-events-none fixed bottom-4 right-4 z-50 flex w-[min(380px,calc(100vw-2rem))] flex-col gap-2" aria-live="polite">
      {toasts.map((t) => (
        <div
          key={t.id}
          className={
            "pointer-events-auto flex items-start gap-3 border bg-panel-2 px-3 py-2 font-mono text-[12px] " +
            (t.tone === "error" ? "border-err/60 text-err" : t.tone === "ok" ? "border-ok/50 text-ok" : "border-line-strong text-fg")
          }
        >
          <span className="flex-1 break-words">{t.message}</span>
          <button type="button" aria-label="Dismiss" className="text-muted hover:text-fg" onClick={() => dismiss(t.id)}>
            ✕
          </button>
        </div>
      ))}
    </div>
  );
}
