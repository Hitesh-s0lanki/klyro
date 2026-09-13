# Klyro frontend

The marketing site and documentation for [Klyro](../README.md), built with
Next.js 15 (App Router), React 19, TypeScript, Tailwind CSS v4, and
[shadcn/ui](https://ui.shadcn.com).

## Develop

```sh
npm install
npm run dev        # http://localhost:3000
npm run build      # production build
npm start          # serve the production build
npm run typecheck  # tsc --noEmit
```

## Layout

```text
src/
  app/
    layout.tsx              root shell: fonts, metadata, header, footer
    page.tsx                the marketing home page
    globals.css             theme tokens, shadcn variables, docs prose
    icon.svg                favicon
    not-found.tsx           404
    docs/
      layout.tsx            sidebar + article + on-this-page shell
      page.tsx              /docs — Introduction
      <slug>/page.tsx       one directory per documentation page
  components/
    layout/                 Header, Footer, Logo
    home/                   one component per marketing section
    docs/                   Sidebar, TableOfContents, PageNav, Callout,
                            CardGrid, RefTable, Steps, DocHeader
    ui/                     shadcn primitives (lowercase filenames) plus
                            CodeBlock, CodeTabs, CopyButton, Section,
                            brand-icons
  content/
    home.ts                 all marketing copy and code samples
    docs/navigation.ts      the documentation sidebar tree
  lib/
    site.ts                 product constants, nav, headline stats
    highlight.ts            dependency-free code tokenizer
    utils.ts                re-exports cn, plus slugify()
```

Content is kept out of the components. Marketing copy lives in
`src/content/home.ts`, the sidebar tree in
`src/content/docs/navigation.ts`, and product constants in `src/lib/site.ts`,
so text changes never mean touching layout code.

## shadcn/ui

`components.json` holds the generator config: the `base-nova` style, the
`neutral` base color, CSS variables, and lucide as the icon library. Add
more primitives the usual way, and they land in `src/components/ui`:

```sh
npx shadcn@latest add dialog tooltip
```

Two things about this install are worth knowing before editing:

- **It is built on [Base UI](https://base-ui.com), not Radix.** Primitives
  take a `render` prop instead of `asChild`, and a button that renders as a
  link must also opt out of native button semantics, or Base UI logs a
  console error:
  `<Button nativeButton={false} render={<Link href="/docs" />}>Docs</Button>`.
  Base UI then emits an anchor carrying `role="button"` and `tabindex="0"`.
  Other props differ from the Radix-era docs too: the accordion takes
  `multiple`, not `openMultiple`.
- **`cn` comes from the `cn` package.** shadcn's own files import it from
  `"cn"`; everything else imports it from `@/lib/utils`, which re-exports it
  alongside `slugify`.

Primitives in use: button, badge, card, table, tabs, accordion, input,
separator, alert, sheet.

## Theme

The site is light-only. `:root` in `globals.css` maps every shadcn token
onto the Klyro palette, so `bg-card`, `text-muted-foreground`, and
`bg-primary` all resolve to the same colors as the custom
`ink`/`surface`/`brand` tokens defined in `@theme` above it. Change a color
once there and both systems follow.

The palette is ordered by elevation rather than by lightness: `canvas` is
the page ground, `surface` the card raised off it, and `surface-2` the strip
recessed back into a card. A component picks the one that matches its depth
and stays correct if the palette is retuned.

Three constraints that are easy to trip over:

- `@custom-variant dark` stays bound to an explicit `.dark` ancestor that
  nothing sets. That is what keeps the `dark:` utilities inside the shadcn
  primitives inert. Delete the line and Tailwind's stock variant takes over,
  which fires off the visitor's OS preference and half-darkens the site.
- `--font-sans` is declared in `:root`, not in `@theme`, because shadcn's
  `@theme inline` block re-declares that key and an inline theme value is
  not emitted as a variable. Declaring it in `@theme` leaves the family
  undefined at runtime and the whole site falls back to a serif.
- `npx shadcn@latest init` rewrites `layout.tsx` to add its own font, and
  drops whatever was on the `<html>` className. Re-check that the font
  variables are still there after running it, and that it did not add
  `dark` back.

## Adding a documentation page

1. Create `src/app/docs/<slug>/page.tsx`, export `metadata`, and render
   `DocHeader` followed by the body.
2. Add the page to `docsNav` in `src/content/docs/navigation.ts`. The
   sidebar, the previous/next footer links, and the filter box all read
   from that one array.
3. Give every `h2`/`h3` an `id`. The on-this-page rail reads the headings
   out of the rendered article, so no separate table of contents is
   maintained.

## Placeholders

Real product behaviour is documented from the server and published packages.
One group of links remains deliberately dummy and is labelled in source:

- **Social and site URLs** in `src/lib/site.ts` (`site.url`, `site.social`).

## Notes

- Syntax highlighting is a small tokenizer in `src/lib/highlight.ts`
  rather than a dependency. Swapping in Shiki means changing that file,
  `CodeBlock`, and `CodeTabs` only.
- Every page is statically prerendered. The client components are the
  header, the code tabs, the copy buttons, the sidebar filter, the
  on-this-page rail, the previous/next links, and the shadcn primitives
  that need state.
