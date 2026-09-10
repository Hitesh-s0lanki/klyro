import Link from "next/link";
import { Logo } from "./Logo";
import { GithubIcon } from "@/components/ui/brand-icons";
import { Separator } from "@/components/ui/separator";
import { footerNav, site } from "@/lib/site";

export function Footer() {
  return (
    <footer className="border-t border-line-soft bg-surface/40">
      <div className="mx-auto w-full max-w-6xl px-5 py-14 sm:px-8">
        <div className="grid gap-10 md:grid-cols-[1.4fr_repeat(3,1fr)]">
          <div>
            <Logo />
            <p className="mt-4 max-w-xs text-[13.5px] leading-relaxed text-ink-muted">
              An in-memory data server with a native memory type for AI agents.
              Open source, single binary, Redis-compatible wire protocol.
            </p>
            <a
              href={site.repo}
              target="_blank"
              rel="noreferrer"
              className="mt-5 inline-flex items-center gap-2 text-[13px] text-ink-muted transition hover:text-ink"
            >
              <GithubIcon className="size-4" /> Star on GitHub
            </a>
          </div>

          {footerNav.map((group) => (
            <div key={group.title}>
              <h3 className="text-[12px] font-semibold uppercase tracking-[0.14em] text-ink-faint">
                {group.title}
              </h3>
              <ul className="mt-4 space-y-2.5">
                {group.links.map((link) => (
                  <li key={link.label}>
                    <Link
                      href={link.href}
                      className="text-[13.5px] text-ink-muted transition hover:text-ink"
                    >
                      {link.label}
                    </Link>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </div>

        <Separator className="mt-12 bg-line-soft" />

        <div className="flex flex-col gap-3 pt-6 text-[12.5px] text-ink-faint sm:flex-row sm:items-center sm:justify-between">
          <p>© {new Date().getFullYear()} Klyro. Released under the MIT license.</p>
          <p className="font-mono">
            v0.1.0 · default port {site.defaultPort} · RESP2 / RESP3
          </p>
        </div>
      </div>
    </footer>
  );
}
