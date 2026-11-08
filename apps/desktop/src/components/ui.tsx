import { useId, type ButtonHTMLAttributes, type InputHTMLAttributes, type ReactNode } from "react";
import { ArrowLeft } from "lucide-react";
import { useNavigate } from "react-router";
import { useTranslation } from "react-i18next";

/* Small primitives in the Silo web-app look (tokens from styles/tokens.css). */

type Variant = "primary" | "secondary" | "ghost" | "danger";

const variants: Record<Variant, string> = {
  primary: "bg-primary text-primary-foreground hover:opacity-90",
  secondary: "bg-secondary text-foreground hover:bg-surface-hover",
  ghost: "text-muted-foreground hover:bg-surface-hover hover:text-foreground",
  danger: "text-destructive hover:bg-surface-hover",
};

export function Button({
  variant = "primary",
  className = "",
  type = "button",
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return (
    <button
      type={type}
      className={`rounded-lg px-4 py-2 text-sm font-medium transition focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring disabled:cursor-not-allowed disabled:opacity-60 ${variants[variant]} ${className}`}
      {...rest}
    />
  );
}

export function Field({
  label,
  hint,
  ...rest
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string }) {
  const id = useId();
  return (
    <div className="flex flex-col gap-1.5 text-sm">
      <label htmlFor={id} className="font-medium">
        {label}
      </label>
      <input
        id={id}
        aria-describedby={hint ? `${id}-hint` : undefined}
        className="rounded-lg border border-border bg-background px-3 py-2 text-base text-foreground placeholder:text-muted-foreground focus:border-ring focus:outline-none"
        {...rest}
      />
      {hint ? (
        <span id={`${id}-hint`} className="text-xs text-muted-foreground">
          {hint}
        </span>
      ) : null}
    </div>
  );
}

export function Alert({
  tone = "error",
  onRetry,
  children,
}: {
  tone?: "error" | "warning" | "info";
  /** Shows a Retry button (load errors). */
  onRetry?: () => void;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  const toneClass =
    tone === "error"
      ? "border-destructive/50 text-destructive"
      : tone === "warning"
        ? "border-amber-400/40 text-amber-200"
        : "border-border text-muted-foreground";
  return (
    <div
      role={tone === "error" ? "alert" : "status"}
      className={`flex items-center justify-between gap-3 rounded-lg border px-3 py-2 text-sm ${toneClass}`}
    >
      <p>{children}</p>
      {onRetry ? (
        <button
          type="button"
          onClick={onRetry}
          className="shrink-0 rounded-md border border-current px-2 py-0.5 text-xs font-medium hover:bg-surface-hover focus-visible:outline-2 focus-visible:outline-ring"
        >
          {t("status.retry")}
        </button>
      ) : null}
    </div>
  );
}

export function Page({ title, children }: { title: string; children: ReactNode }) {
  return (
    <main className="flex h-full flex-col items-center overflow-y-auto p-8">
      <div className="flex w-full max-w-xl flex-col gap-6">
        <h1 className="text-3xl font-semibold tracking-tight">{title}</h1>
        {children}
      </div>
    </main>
  );
}

export function Card({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <section className={`rounded-lg border border-border bg-card p-6 ${className}`}>{children}</section>;
}

/** "Back" pill at the top of a page: the previous screen, or `fallback` when the page was opened directly. */
export function BackButton({ fallback }: { fallback: string }) {
  const { t } = useTranslation();
  const navigate = useNavigate();
  return (
    <button
      type="button"
      onClick={() => {
        const idx = (window.history.state as { idx?: number } | null)?.idx ?? 0;
        if (idx > 0) void navigate(-1);
        else void navigate(fallback);
      }}
      className="flex h-9 items-center gap-1.5 self-start rounded-full border border-border bg-black/20 pr-4 pl-3 text-sm font-medium text-muted-foreground backdrop-blur transition hover:border-foreground/40 hover:text-foreground focus-visible:outline-2 focus-visible:outline-ring"
    >
      <ArrowLeft className="size-4" />
      {t("item.back")}
    </button>
  );
}
