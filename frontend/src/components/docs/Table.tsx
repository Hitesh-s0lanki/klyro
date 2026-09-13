import type { ReactNode } from "react";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";

/**
 * The reference tables throughout the docs. shadcn's Table already
 * supplies the horizontal scroll container, so a wide command
 * signature never breaks the page.
 */
export function RefTable({
  head,
  rows,
  monoFirst = true,
}: {
  head: string[];
  rows: ReactNode[][];
  monoFirst?: boolean;
}) {
  return (
    <div className="scroll-slim my-6 overflow-hidden rounded-card ring-1 ring-line">
      <Table className="min-w-[560px]">
        <TableHeader>
          <TableRow className="border-line bg-surface-2/50 hover:bg-surface-2/50">
            {head.map((cell) => (
              <TableHead
                key={cell}
                className="h-auto px-4 py-3 text-[11.5px] font-semibold uppercase tracking-[0.12em] text-ink-faint"
              >
                {cell}
              </TableHead>
            ))}
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row, rowIndex) => (
            <TableRow key={rowIndex} className="border-line-soft align-top hover:bg-surface-2/50">
              {row.map((cell, cellIndex) => (
                <TableCell
                  key={cellIndex}
                  className={
                    cellIndex === 0 && monoFirst
                      ? "px-4 py-3 font-mono text-[12.5px] leading-relaxed whitespace-normal text-ink"
                      : "px-4 py-3 text-[13.5px] leading-relaxed whitespace-normal text-ink-muted"
                  }
                >
                  {cell}
                </TableCell>
              ))}
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  );
}
