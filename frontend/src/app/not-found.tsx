import Link from "next/link";
import { ArrowRightIcon } from "lucide-react";
import { Button } from "@/components/ui/button";

export default function NotFound() {
  return (
    <div className="mx-auto flex min-h-[70vh] w-full max-w-2xl flex-col items-center justify-center px-5 text-center">
      <p className="font-mono text-[11px] uppercase tracking-[0.2em] text-brand-bright">
        Error 404
      </p>
      <h1 className="mt-4 text-3xl font-semibold tracking-tight text-ink sm:text-4xl">
        Nothing stored under that key
      </h1>
      <p className="mt-4 max-w-md text-[15px] leading-relaxed text-ink-muted">
        The page you asked for does not exist. Try the documentation index, or
        head back to the start.
      </p>
      <div className="mt-8 flex flex-col gap-3 sm:flex-row">
        <Button size="lg" nativeButton={false} render={<Link href="/docs" />}>
          Browse the docs <ArrowRightIcon />
        </Button>
        <Button size="lg" variant="outline" nativeButton={false} render={<Link href="/" />}>
          Back to home
        </Button>
      </div>
    </div>
  );
}
