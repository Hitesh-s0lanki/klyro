/** Single source of truth for product copy used in more than one place. */
export const site = {
  name: "Klyro",
  tagline: "A fast in-memory database with a familiar Redis interface",
  description:
    "Klyro is an in-memory database for strings, lists, hashes, sets, sorted sets, queues, pub/sub, transactions, and ranked text and vector search. It speaks RESP, so existing Redis clients can connect directly.",
  url: "https://github.com/Hitesh-s0lanki/klyro",
  repo: "https://github.com/Hitesh-s0lanki/klyro",
  docker: "ghcr.io/hitesh-s0lanki/klyro:latest",
  defaultPort: 7171,
  npm: "https://www.npmjs.com/package/klyro-db",
  pypi: "https://pypi.org/project/klyro-db/",
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
      { label: "Data types", href: "/docs/data-types" },
      { label: "Memory indexes", href: "/docs/memory-indexes" },
    ],
  },
  {
    title: "Resources",
    links: [
      { label: "SDKs & packages", href: "/docs/sdks" },
      { label: "Configuration", href: "/docs/configuration" },
      { label: "Clients", href: "/docs/clients" },
      { label: "Roadmap", href: "/docs/roadmap" },
      { label: "npm", href: "https://www.npmjs.com/package/klyro-db" },
      { label: "PyPI", href: "https://pypi.org/project/klyro-db/" },
      { label: "GitHub", href: "https://github.com/Hitesh-s0lanki/klyro" },
    ],
  },
] as const;

/** Headline numbers, all taken from the current build of the server. */
export const stats = [
  { value: "145", label: "commands", detail: "130 Redis-shaped, 15 MEM.*" },
  { value: "6", label: "data types", detail: "five classic, one searchable" },
  { value: "~15 MB", label: "container image", detail: "static musl on Alpine" },
  { value: "1", label: "dependency", detail: "libc, for poll()" },
] as const;
