"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";
import { useEffect, useState } from "react";
import { MenuIcon } from "lucide-react";
import { Logo } from "./Logo";
import { Button } from "@/components/ui/button";
import { GithubIcon } from "@/components/ui/brand-icons";
import {
  Sheet,
  SheetContent,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import { primaryNav, site } from "@/lib/site";
import { cn } from "@/lib/utils";

export function Header() {
  const [scrolled, setScrolled] = useState(false);
  const [open, setOpen] = useState(false);
  const pathname = usePathname();

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 8);
    onScroll();
    window.addEventListener("scroll", onScroll, { passive: true });
    return () => window.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => setOpen(false), [pathname]);

  return (
    <header
      className={cn(
        "sticky top-0 z-50 border-b transition-colors duration-300",
        scrolled
          ? "border-line-soft bg-canvas/85 backdrop-blur-xl"
          : "border-transparent bg-transparent",
      )}
    >
      <div className="mx-auto flex h-16 w-full max-w-7xl items-center justify-between gap-6 px-5 sm:px-8">
        <div className="flex items-center gap-8">
          <Logo />
          <nav className="hidden items-center gap-1 md:flex">
            {primaryNav.map((item) => (
              <Button
                key={item.href}
                variant="ghost"
                size="sm"
                nativeButton={false}
                render={<Link href={item.href} />}
                className={cn(
                  "text-[13.5px] font-normal",
                  pathname.startsWith("/docs") && item.href === "/docs"
                    ? "text-ink"
                    : "text-ink-muted",
                )}
              >
                {item.label}
              </Button>
            ))}
          </nav>
        </div>

        <div className="hidden items-center gap-2 md:flex">
          <Button
            variant="ghost"
            size="sm"
            nativeButton={false}
            render={<a href={site.repo} target="_blank" rel="noreferrer" />}
            className="font-normal text-ink-muted"
          >
            <GithubIcon className="size-4" />
            GitHub
          </Button>
          <Button size="lg" nativeButton={false} render={<Link href="/docs/quickstart" />}>
            Get started
          </Button>
        </div>

        <Sheet open={open} onOpenChange={setOpen}>
          <SheetTrigger
            render={<Button variant="outline" size="icon" className="md:hidden" />}
            aria-label="Open menu"
          >
            <MenuIcon />
          </SheetTrigger>
          <SheetContent side="right" className="w-[17rem] max-w-[85vw] bg-surface">
            <SheetHeader>
              <SheetTitle>Klyro</SheetTitle>
            </SheetHeader>
            <nav className="flex flex-col gap-1 px-4">
              {primaryNav.map((item) => (
                <Link
                  key={item.href}
                  href={item.href}
                  onClick={() => setOpen(false)}
                  className="rounded-md px-2 py-2.5 text-sm text-ink-muted transition hover:bg-surface-2 hover:text-ink"
                >
                  {item.label}
                </Link>
              ))}
            </nav>
            <div className="mt-auto flex flex-col gap-2 p-4">
              <Button size="lg" nativeButton={false} render={<Link href="/docs/quickstart" />}>
                Get started
              </Button>
              <Button
                size="lg"
                variant="outline"
                nativeButton={false}
                render={<a href={site.repo} target="_blank" rel="noreferrer" />}
              >
                <GithubIcon className="size-4" /> GitHub
              </Button>
            </div>
          </SheetContent>
        </Sheet>
      </div>
    </header>
  );
}
