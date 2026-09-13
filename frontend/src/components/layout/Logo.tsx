import Link from "next/link";
import Image from "next/image";
import { cn } from "@/lib/utils";

export function Logo({ className, href = "/" }: { className?: string; href?: string }) {
  return (
    <Link href={href} className={cn("group inline-flex items-center gap-2.5", className)}>
      <Image
        src="/logos/brand/kylro-logo.png"
        alt="Kylro"
        width={224}
        height={92}
        priority
        className="h-9 w-auto object-contain transition-transform group-hover:scale-[1.02]"
      />
    </Link>
  );
}
