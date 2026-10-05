import { Hero } from "@/components/home/Hero";
import { Features } from "@/components/home/Features";
import { HowItWorks } from "@/components/home/HowItWorks";
import { Benefits } from "@/components/home/Benefits";
import { Comparison } from "@/components/home/Comparison";
import { Packages } from "@/components/home/Packages";
import { Faq } from "@/components/home/Faq";
import { CallToAction } from "@/components/home/CallToAction";

export default function HomePage() {
  return (
    <>
      <Hero />
      <Features />
      <HowItWorks />
      <Benefits />
      <Comparison />
      <Packages />
      <Faq />
      <CallToAction />
    </>
  );
}
