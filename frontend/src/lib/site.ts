/**
 * Single source of truth for product copy that appears in more than one
 * place. Placeholder URLs are marked so they are easy to find and swap
 * once the real endpoints exist.
 */
export const site = {
  name: "Klyro",
  tagline: "The memory layer for AI agents",
  description:
    "Klyro is an in-memory data server that stores text and embeddings in one index and ranks them by keyword relevance, semantic similarity, recency, and importance in a single query. It speaks the Redis wire protocol, so the client you already have works.",
  url: "https://klyro.dev", // placeholder
  repo: "https://github.com/Hitesh-s0lanki/klyro",
  docker: "ghcr.io/hitesh-s0lanki/klyro:latest",
  defaultPort: 7171,
  social: {
    x: "https://x.com/klyrodb", // placeholder
    discord: "https://discord.gg/klyro", // placeholder
    npm: "https://www.npmjs.com/package/@klyro/client", // placeholder
  },
} as const;

export const primaryNav = [
  { label: "Features", href: "/#features" },
  { label: "How it works", href: "/#how-it-works" },
  { label: "Why Klyro", href: "/#why-klyro" },
  { label: "SDKs", href: "/#sdks" },
  { label: "Docs", href: "/docs" },
] as const;

export const footerNav = [
  {
    title: "Product",
    links: [
      { label: "Features", href: "/#features" },
      { label: "How it works", href: "/#how-it-works" },
      { label: "Why Klyro", href: "/#why-klyro" },
      { label: "Comparison", href: "/#comparison" },
      { label: "FAQ", href: "/#faq" },
    ],
  },
  {
    title: "Documentation",
    links: [
      { label: "Introduction", href: "/docs" },
      { label: "Quickstart", href: "/docs/quickstart" },
      { label: "Installation", href: "/docs/installation" },
      { label: "Memory indexes", href: "/docs/memory-indexes" },
      { label: "MEM.* reference", href: "/docs/api-reference" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "SDKs & packages", href: "/docs/sdks" },
      { label: "Configuration", href: "/docs/configuration" },
      { label: "Clients", href: "/docs/clients" },
      { label: "Roadmap", href: "/docs/roadmap" },
      { label: "GitHub", href: "https://github.com/Hitesh-s0lanki/klyro" },
    ],
  },
] as const;

/** Headline numbers, all taken from the current build of the server. */
export const stats = [
  { value: "122", label: "commands", detail: "107 Redis-shaped, 15 MEM.*" },
  { value: "382", label: "tests", detail: "unit plus real-socket integration" },
  { value: "~15 MB", label: "container image", detail: "static musl on Alpine" },
  { value: "1", label: "dependency", detail: "libc, for poll()" },
] as const;
