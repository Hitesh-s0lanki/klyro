import type { Metadata } from "next";
import { Sidebar } from "@/components/docs/Sidebar";
import { MobileSidebar } from "@/components/docs/MobileSidebar";
import { TableOfContents } from "@/components/docs/TableOfContents";
import { PageNav } from "@/components/docs/PageNav";

export const metadata: Metadata = {
  title: {
    default: "Documentation",
    template: "%s · Klyro docs",
  },
};

export default function DocsLayout({ children }: { children: React.ReactNode }) {
  return (
    <div className="mx-auto w-full max-w-7xl px-5 sm:px-8">
      <div className="lg:grid lg:grid-cols-[16rem_minmax(0,1fr)] lg:gap-10 xl:grid-cols-[16rem_minmax(0,1fr)_14rem] xl:gap-12">
        <aside className="hidden lg:sticky lg:top-16 lg:block lg:h-[calc(100vh-4rem)] lg:border-r lg:border-line-soft lg:py-8 lg:pr-6">
          <Sidebar />
        </aside>

        <div className="min-w-0 py-8 lg:py-12">
          <div className="mb-6 lg:hidden">
            <MobileSidebar />
          </div>
          <article className="doc-body">{children}</article>
          <PageNav />
        </div>

        <aside className="hidden xl:sticky xl:top-16 xl:block xl:h-[calc(100vh-4rem)] xl:overflow-y-auto xl:py-12">
          <TableOfContents />
        </aside>
      </div>
    </div>
  );
}
