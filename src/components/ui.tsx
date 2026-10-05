import { useEffect, useRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import type { Tone } from "../lib/format";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

const TONE_TEXT: Record<Tone, string> = {
  ok: "text-ok",
  busy: "text-clay",
  warn: "text-warn",
  error: "text-err",
  idle: "text-muted",
};

const TONE_BG: Record<Tone, string> = {
  ok: "bg-ok",
  busy: "bg-clay",
  warn: "bg-warn",
  error: "bg-err",
  idle: "bg-dim",
};

export function toneText(tone: Tone): string {
  return TONE_TEXT[tone];
}

/** Square indicator lamp. Pulses while a process is live. */
export function StatusLight({ tone, pulse = false, title }: { tone: Tone; pulse?: boolean; title?: string }) {
  return (
    <span
      role="img"
      aria-label={title ?? tone}
      title={title}
      className={cx("inline-block size-2 shrink-0", TONE_BG[tone], pulse && "tk-pulse")}
      style={{ boxShadow: tone === "idle" ? undefined : "0 0 6px currentColor" }}
    />
  );
}

/** Small uppercase mono system label, e.g. "STATUS". */
export function Label({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cx("font-mono text-[10px] uppercase tracking-[0.14em] text-muted", className)}>{children}</div>
  );
}

export function StateTag({ tone, children }: { tone: Tone; children: ReactNode }) {
  return (
    <span className={cx("inline-flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-wider", TONE_TEXT[tone])}>
      <StatusLight tone={tone} pulse={tone === "busy"} />
      {children}
    </span>
  );
}

type Variant = "default" | "primary" | "danger" | "ghost";

const VARIANTS: Record<Variant, string> = {
  default: "border-line-strong text-fg hover:border-fg/60 hover:bg-panel-2",
  primary: "border-clay text-clay hover:bg-clay hover:text-bg",
  danger: "border-err/60 text-err hover:bg-err hover:text-bg",
  ghost: "border-transparent text-muted hover:text-fg hover:border-line",
};

export function Button({
  variant = "default",
  size = "md",
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: "sm" | "md" }) {
  return (
    <button
      type="button"
      {...rest}
      className={cx(
        "inline-flex select-none items-center justify-center gap-2 border font-mono uppercase tracking-wider transition-colors",
        "disabled:cursor-not-allowed disabled:opacity-35 disabled:hover:bg-transparent",
        size === "sm" ? "h-7 px-2 text-[10.5px]" : "h-8 px-3 text-[11px]",
        VARIANTS[variant],
        className,
      )}
    >
      {children}
    </button>
  );
}

/** Icon-only square button with an accessible label. */
export function IconButton({ label, children, ...rest }: ButtonHTMLAttributes<HTMLButtonElement> & { label: string }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      {...rest}
      className={cx(
        "inline-flex size-7 items-center justify-center border border-transparent font-mono text-[12px] text-muted",
        "hover:border-line hover:text-fg disabled:cursor-not-allowed disabled:opacity-30 disabled:hover:border-transparent",
        rest.className,
      )}
    >
      {children}
    </button>
  );
}

export function Panel({
  title,
  right,
  children,
  className,
  bodyClassName,
}: {
  title: ReactNode;
  right?: ReactNode;
  children: ReactNode;
  className?: string;
  bodyClassName?: string;
}) {
  return (
    <section className={cx("flex min-h-0 flex-col border border-line bg-panel", className)}>
      <header className="flex h-9 shrink-0 items-center justify-between gap-3 border-b border-line px-3">
        <h2 className="font-mono text-[11px] font-medium uppercase tracking-[0.16em] text-fg/90">{title}</h2>
        {right && <div className="flex items-center gap-2">{right}</div>}
      </header>
      <div className={cx("min-h-0 flex-1", bodyClassName)}>{children}</div>
    </section>
  );
}

/** Centered message used for empty, loading, and error states. */
export function EmptyState({
  title,
  children,
  tone = "idle",
  action,
}: {
  title: string;
  children?: ReactNode;
  tone?: Tone;
  action?: ReactNode;
}) {
  return (
    <div className="flex h-full min-h-28 flex-col items-center justify-center gap-2 px-6 py-8 text-center">
      <div className={cx("font-mono text-[11px] uppercase tracking-[0.18em]", TONE_TEXT[tone])}>{title}</div>
      {children && <div className="max-w-md text-[12.5px] text-muted">{children}</div>}
      {action && <div className="mt-2">{action}</div>}
    </div>
  );
}

export function Modal({
  open,
  title,
  onClose,
  children,
  footer,
  width = "max-w-2xl",
}: {
  open: boolean;
  title: ReactNode;
  onClose: () => void;
  children: ReactNode;
  footer?: ReactNode;
  width?: string;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const prev = document.activeElement as HTMLElement | null;
    const first = ref.current?.querySelector<HTMLElement>("input, textarea, select, button");
    first?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      prev?.focus();
    };
  }, [open, onClose]);
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex items-start justify-center overflow-y-auto bg-black/75 p-4 pt-[8vh]">
      <div
        ref={ref}
        role="dialog"
        aria-modal="true"
        className={cx("w-full border border-line-strong bg-panel shadow-[0_0_0_1px_rgba(0,0,0,0.6)]", width)}
      >
        <header className="flex h-10 items-center justify-between border-b border-line px-4">
          <h2 className="font-mono text-[11px] uppercase tracking-[0.16em]">{title}</h2>
          <IconButton label="Close" onClick={onClose}>
            ✕
          </IconButton>
        </header>
        <div className="max-h-[70vh] overflow-y-auto p-4">{children}</div>
        {footer && <footer className="flex flex-wrap justify-end gap-2 border-t border-line px-4 py-3">{footer}</footer>}
      </div>
    </div>
  );
}

const INPUT =
  "w-full border border-line bg-bg px-2.5 py-1.5 text-[13px] text-fg placeholder:text-dim focus:border-clay focus:outline-none";

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <label className="block space-y-1.5">
      <Label>{label}</Label>
      {children}
      {hint && <div className="text-[11.5px] text-muted">{hint}</div>}
    </label>
  );
}

export function TextInput(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input {...props} className={cx(INPUT, "h-8", props.className)} />;
}

export function TextArea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea {...props} className={cx(INPUT, "font-mono text-[12.5px] leading-relaxed", props.className)} />;
}

export function Select(props: React.SelectHTMLAttributes<HTMLSelectElement>) {
  return <select {...props} className={cx(INPUT, "h-8", props.className)} />;
}

/** ON/OFF switch rendered as a two-state hardware toggle. */
export function Toggle({
  checked,
  onChange,
  label,
  description,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  description?: string;
  disabled?: boolean;
}) {
  return (
    <div className="flex items-start justify-between gap-4 py-2">
      <div>
        <div className="text-[13px]">{label}</div>
        {description && <div className="text-[11.5px] text-muted">{description}</div>}
      </div>
      <button
        type="button"
        role="switch"
        aria-checked={checked}
        aria-label={label}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className="flex h-6 shrink-0 border border-line-strong font-mono text-[10px] disabled:opacity-40"
      >
        <span className={cx("flex w-9 items-center justify-center", checked ? "bg-ok/15 text-ok" : "text-dim")}>ON</span>
        <span className={cx("flex w-9 items-center justify-center border-l border-line-strong", !checked ? "bg-panel-2 text-fg" : "text-dim")}>
          OFF
        </span>
      </button>
    </div>
  );
}
