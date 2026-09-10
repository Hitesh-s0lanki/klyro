import {
  Accordion,
  AccordionContent,
  AccordionItem,
  AccordionTrigger,
} from "@/components/ui/accordion";
import { Section, SectionHeading } from "@/components/ui/Section";
import { faq } from "@/content/home";

export function Faq() {
  return (
    <Section id="faq">
      <div className="grid gap-12 md:grid-cols-[minmax(0,0.8fr)_minmax(0,1.2fr)]">
        <SectionHeading
          eyebrow="FAQ"
          title="Questions people ask first"
          description="If something here is still unclear, the documentation goes deeper on every point."
        />

        <Accordion multiple={false} className="border-y border-line-soft">
          {faq.map((item) => (
            <AccordionItem key={item.q} value={item.q} className="border-line-soft">
              <AccordionTrigger className="py-5 text-[15px] font-medium text-ink hover:no-underline">
                {item.q}
              </AccordionTrigger>
              <AccordionContent className="max-w-2xl pb-5 text-[13.8px] leading-relaxed text-ink-muted">
                {item.a}
              </AccordionContent>
            </AccordionItem>
          ))}
        </Accordion>
      </div>
    </Section>
  );
}
