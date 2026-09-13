import { CheckIcon, MinusIcon } from "lucide-react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { Section, SectionHeading } from "@/components/ui/Section";
import { comparison } from "@/content/home";
import { cn } from "@/lib/utils";

function Cell({ value, primary }: { value: boolean | string; primary: boolean }) {
  if (value === true) {
    return (
      <span
        className={cn("inline-flex items-center gap-1.5 text-[13px]", primary ? "text-mint" : "text-ink")}
      >
        <CheckIcon className="size-4" /> Yes
      </span>
    );
  }
  if (value === false) {
    return (
      <span className="inline-flex items-center gap-1.5 text-[13px] text-ink-faint">
        <MinusIcon className="size-4" /> No
      </span>
    );
  }
  return <span className="text-[13px] text-ink-muted">{value}</span>;
}

export function Comparison() {
  return (
    <Section id="comparison">
      <SectionHeading
        eyebrow="Comparison"
        title="Core data structures, with search when you need it"
        description="Use the standard keyspace for application state and coordination. Add a memory index when records need keyword, vector, recency, or importance ranking."
      />

      <div className="scroll-slim mt-12 overflow-hidden rounded-card ring-1 ring-line">
        <Table className="min-w-[640px]">
          <TableHeader>
            <TableRow className="border-line bg-surface-2/50 hover:bg-surface-2/50">
              <TableHead className="h-auto px-5 py-4 text-[12px] font-semibold uppercase tracking-[0.12em] text-ink-faint">
                Capability
              </TableHead>
              {comparison.columns.map((column, index) => (
                <TableHead
                  key={column}
                  className={cn(
                    "h-auto px-5 py-4 text-[13px] font-semibold",
                    index === 0 ? "text-brand-bright" : "text-ink-muted",
                  )}
                >
                  {column}
                </TableHead>
              ))}
            </TableRow>
          </TableHeader>
          <TableBody>
            {comparison.rows.map((row) => (
              <TableRow key={row.label} className="border-line-soft hover:bg-surface-2/50">
                <TableCell className="px-5 py-3.5 text-[13.5px] font-medium whitespace-normal text-ink">
                  {row.label}
                </TableCell>
                {row.values.map((value, index) => (
                  <TableCell
                    key={index}
                    className={cn("px-5 py-3.5 whitespace-normal", index === 0 && "bg-brand/[0.04]")}
                  >
                    <Cell value={value} primary={index === 0} />
                  </TableCell>
                ))}
              </TableRow>
            ))}
          </TableBody>
        </Table>
      </div>
    </Section>
  );
}
