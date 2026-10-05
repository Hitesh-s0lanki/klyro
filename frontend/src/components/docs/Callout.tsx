import type { ReactNode } from "react";
import { InfoIcon, LightbulbIcon, OctagonAlertIcon, TriangleAlertIcon } from "lucide-react";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { cn } from "@/lib/utils";

type Variant = "note" | "tip" | "warning" | "danger";

const STYLES: Record<
  Variant,
  { wrap: string; accent: string; title: string; icon: typeof InfoIcon }
> = {
  note: { wrap: "ring-line", accent: "text-cyan", title: "Note", icon: InfoIcon },
  tip: { wrap: "bg-mint/[0.05] ring-mint/25", accent: "text-mint", title: "Tip", icon: LightbulbIcon },
  warning: {
    wrap: "bg-amber/[0.05] ring-amber/25",
    accent: "text-amber",
    title: "Careful",
    icon: TriangleAlertIcon,
  },
  danger: {
    wrap: "bg-destructive/[0.06] ring-destructive/25",
    accent: "text-destructive",
    title: "Warning",
    icon: OctagonAlertIcon,
  },
};

export function Callout({
  variant = "note",
  title,
  children,
}: {
  variant?: Variant;
  title?: string;
  children: ReactNode;
}) {
  const style = STYLES[variant];
  const Icon = style.icon;

  return (
    <Alert className={cn("my-6 gap-1 rounded-card px-5 py-4 ring-1", style.wrap)}>
      <Icon className={style.accent} />
      <AlertTitle
        className={cn("text-[12px] font-semibold uppercase tracking-[0.12em]", style.accent)}
      >
        {title ?? style.title}
      </AlertTitle>
      <AlertDescription className="text-[14px] leading-relaxed text-ink-muted [&>*+*]:mt-2 [&_a]:text-brand-bright">
        {children}
      </AlertDescription>
    </Alert>
  );
}
