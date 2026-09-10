"use client";

import { useState } from "react";
import { TOKEN_CLASS, tokenize, type Language } from "@/lib/highlight";
import { Card } from "@/components/ui/card";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { cn } from "@/lib/utils";
import { CopyButton } from "./CopyButton";

export type CodeTab = {
  label: string;
  lang: Language;
  code: string;
};

export function CodeTabs({ tabs, className }: { tabs: CodeTab[]; className?: string }) {
  const [active, setActive] = useState(tabs[0].label);
  const current = tabs.find((tab) => tab.label === active) ?? tabs[0];

  return (
    <Card className={cn("gap-0 rounded-card py-0", className)}>
      <Tabs
        value={active}
        onValueChange={(value) => setActive(String(value))}
        className="gap-0"
      >
        <div className="flex items-center justify-between gap-3 border-b border-line-soft bg-surface-2/60 px-2 py-1.5">
          <TabsList variant="line" className="scroll-slim h-8 overflow-x-auto">
            {tabs.map((tab) => (
              <TabsTrigger
                key={tab.label}
                value={tab.label}
                className="whitespace-nowrap px-3 text-xs data-active:text-brand-bright"
              >
                {tab.label}
              </TabsTrigger>
            ))}
          </TabsList>
          <CopyButton value={current.code.trim()} className="mr-1 shrink-0" />
        </div>

        {tabs.map((tab) => (
          <TabsContent key={tab.label} value={tab.label} className="m-0">
            <pre className="scroll-slim overflow-x-auto px-4 py-4">
              <code className="font-mono text-[12.5px] leading-[1.75]">
                {tokenize(tab.code.trim(), tab.lang).map((token, i) => (
                  <span key={i} className={TOKEN_CLASS[token.type]}>
                    {token.value}
                  </span>
                ))}
              </code>
            </pre>
          </TabsContent>
        ))}
      </Tabs>
    </Card>
  );
}
