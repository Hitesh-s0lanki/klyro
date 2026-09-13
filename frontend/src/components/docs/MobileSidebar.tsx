"use client";

import { useState } from "react";
import { MenuIcon } from "lucide-react";
import { Sidebar } from "./Sidebar";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetHeader, SheetTitle, SheetTrigger } from "@/components/ui/sheet";

export function MobileSidebar() {
  const [open, setOpen] = useState(false);

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <SheetTrigger
        render={<Button variant="outline" size="sm" className="lg:hidden" />}
      >
        <MenuIcon /> Documentation menu
      </SheetTrigger>
      <SheetContent side="right" className="w-[19rem] max-w-[85vw] bg-surface">
        <SheetHeader className="pb-0">
          <SheetTitle>Documentation</SheetTitle>
        </SheetHeader>
        <div className="min-h-0 flex-1 px-4 pb-4">
          <Sidebar onNavigate={() => setOpen(false)} />
        </div>
      </SheetContent>
    </Sheet>
  );
}
