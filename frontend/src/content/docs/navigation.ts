export type DocLink = {
  title: string;
  href: string;
  /** Short label shown as a chip in the sidebar. */
  tag?: string;
};

export type DocGroup = {
  title: string;
  links: DocLink[];
};

export const docsNav: DocGroup[] = [
  {
    title: "Get started",
    links: [
      { title: "Introduction", href: "/docs" },
      { title: "Quickstart", href: "/docs/quickstart" },
      { title: "Installation", href: "/docs/installation" },
    ],
  },
  {
    title: "Core concepts",
    links: [
      { title: "Memory indexes", href: "/docs/memory-indexes" },
      { title: "Retrieval & ranking", href: "/docs/retrieval-and-ranking" },
      { title: "Filters & metadata", href: "/docs/filters" },
      { title: "Persistence", href: "/docs/persistence" },
    ],
  },
  {
    title: "Reference",
    links: [
      { title: "MEM.* commands", href: "/docs/api-reference", tag: "15" },
      { title: "Data type commands", href: "/docs/data-types", tag: "107" },
      { title: "Configuration", href: "/docs/configuration" },
    ],
  },
  {
    title: "Integrations",
    links: [
      { title: "Client libraries", href: "/docs/clients" },
      { title: "SDKs & packages", href: "/docs/sdks", tag: "beta" },
    ],
  },
  {
    title: "About",
    links: [
      { title: "Limitations", href: "/docs/limitations" },
      { title: "Roadmap", href: "/docs/roadmap" },
    ],
  },
];

/** Flattened, in sidebar order, for the previous/next footer links. */
export const docsFlat: DocLink[] = docsNav.flatMap((group) => group.links);
