"use client";

import { useEffect, useState } from "react";
import { CheckIcon, CopyIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

export function CopyButton({
  value,
  className,
  label = true,
}: {
  value: string;
  className?: string;
  /** Show the word alongside the icon. */
  label?: boolean;
}) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 1600);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <Button
      variant="ghost"
      size={label ? "xs" : "icon-xs"}
      aria-label={copied ? "Copied" : "Copy to clipboard"}
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
        } catch {
          setCopied(false);
        }
      }}
      className={cn("text-muted-foreground hover:text-foreground", className)}
    >
      {copied ? <CheckIcon className="text-mint" /> : <CopyIcon />}
      {label ? (copied ? "Copied" : "Copy") : null}
    </Button>
  );
}
