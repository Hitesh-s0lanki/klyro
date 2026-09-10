import { Badge } from "@/components/ui/badge";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { CodeBlock } from "@/components/ui/CodeBlock";
import { CopyButton } from "@/components/ui/CopyButton";
import { Section, SectionHeading } from "@/components/ui/Section";
import { Alert, AlertDescription } from "@/components/ui/alert";
import { packages } from "@/content/home";

export function Packages() {
  return (
    <Section id="sdks">
      <SectionHeading
        eyebrow="Packages & imports"
        title="Install a typed client, or use the Redis client you have"
        description="The wire protocol needs no SDK at all. The optional clients wrap the MEM.* family in a typed surface so vectors, metadata, and filters stop being positional strings."
      />

      <Alert className="mt-4 rounded-lg bg-amber/[0.06] ring-1 ring-amber/25">
        <AlertDescription className="text-[13px] text-ink-muted">
          <span className="font-semibold text-amber">Placeholder:</span> the package
          names and import paths below are reserved but not yet published. Swap them
          for the real coordinates once the SDKs ship.
        </AlertDescription>
      </Alert>

      <div className="mt-12 grid gap-4 lg:grid-cols-3">
        {packages.map((pkg) => (
          <Card key={pkg.name} className="gap-0 rounded-card bg-surface/40 py-0">
            <CardHeader className="grid-cols-none border-b border-line-soft px-5 py-4">
              <div className="flex items-center justify-between gap-3">
                <CardTitle className="font-mono text-[13.5px] font-medium text-ink">
                  {pkg.name}
                </CardTitle>
                <Badge variant="outline" className="text-[10.5px] uppercase tracking-wider text-ink-faint">
                  {pkg.manager}
                </Badge>
              </div>
              <div className="mt-3 flex items-center gap-2 rounded-lg bg-canvas px-3 py-2 ring-1 ring-line">
                <code className="scroll-slim flex-1 overflow-x-auto whitespace-nowrap font-mono text-[12px] text-ink-muted">
                  {pkg.install}
                </code>
                <CopyButton value={pkg.install} label={false} />
              </div>
            </CardHeader>
            <CardContent className="px-0 py-0">
              <CodeBlock
                code={pkg.code}
                lang={pkg.lang}
                copyable={false}
                className="rounded-none bg-transparent ring-0"
              />
            </CardContent>
          </Card>
        ))}
      </div>
    </Section>
  );
}
