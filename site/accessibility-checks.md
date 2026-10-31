# Homepage accessibility checks

Manual checks for `site/index.html` and `site/style.css`. Run them after any
change to the homepage markup or styles. They do not cover the generated
`site/docs/` pages or the wasm demo.

## Setup

- Serve the folder locally, for example `python3 -m http.server --bind
  127.0.0.1 8000 --directory site`, and open `http://127.0.0.1:8000/`.
- Use a private browser profile that is not signed in to anything.
- Check at a desktop width (about 1280×800). Repeat the keyboard and layout
  checks at a narrow width (about 500px), where the header wraps and grows
  taller, and at about 360px, where its links wrap onto a second row and the
  larger scroll padding applies.
- These checks cover keyboard and browser behavior only; they are not a
  substitute for a screen reader or a second browser engine.
- Optionally run [axe-core](https://github.com/dequelabs/axe-core) on the
  page. It should report no violations. It reports text contrast as
  "incomplete" because of the background gradients; use the contrast table
  below for those pairs.

## Keyboard

| Check | Expected result |
| --- | --- |
| Press Tab once on a freshly loaded page. | "Skip to content" appears in the top-left corner with a visible focus ring. |
| Press Enter on "Skip to content", then Tab. | Focus moves to "try it in your browser", the first link in the main content. |
| Tab through the whole page. | Order: Skip to content, ferrule, docs, features, interop, get started, live demo, GitHub, try it in your browser, read the docs, source on GitHub, live demo (card link), the import code block, the get-started code block, footer GitHub link. Nothing is skipped and nothing receives focus twice. |
| Shift+Tab back from the footer to the top. | The same elements in reverse order. |
| Watch every focused element. | Each shows a 2px copper (`--copper-hi`) outline offset from the element, clearly separate from the ghost-button borders. |
| Shift+Tab or Tab into an element that has to scroll into view. | The focused element and its outline end up below the sticky header, not underneath it. Wait for smooth scrolling to finish before judging. |
| Focus a code block and press the arrow keys. | The block scrolls sideways when its lines are wider than the box. |
| Activate "features", "interop" and "get started" in the header. | The page scrolls so the section heading is fully visible below the header. |

## Semantics and names

| Check | Expected result |
| --- | --- |
| Landmarks | One banner (header), one navigation, one main, one contentinfo (footer), and two named regions for the code blocks. |
| Headings | One `h1`, then `h2` for each section and `h3` for each feature card. No levels are skipped. |
| Feature card headings | The accessible name is the text only. The decorative glyphs (⚙, ⇄, ⭑, 🖱, 🧪, 🦀) are hidden from assistive technology. |
| Call-to-action link | Its accessible name is "try it in your browser" without the ▶ glyph. |
| Hero illustration | The SVG has the image role and a name describing the mapping. The figure is named by its visible caption. |
| Code blocks | Each is a focusable region with a short name ("Import and run an .mfd design", "Clone and run ferrule"). |
| Links | Every link has a descriptive visible name and its original destination. |
| Links inside sentences | The card's "live demo" link and the footer link are underlined, not marked by colour alone. |

## Contrast

WCAG 2 ratios computed from the colours in `style.css`. Semi-transparent
colours are blended onto the background they sit on. Normal text needs 4.5:1;
focus outlines, borders and the link-versus-text difference need 3:1.

| Pair | Ratio |
| --- | ---: |
| Body text `--ink` on page background | 14.15:1 |
| `--ink` on the brightest hero glow (`#2a2115`) | 12.09:1 |
| Secondary text `--ink-dim` on page background | 5.85:1 |
| `--ink-dim` on the brightest hero glow | 5.00:1 |
| Header links `--ink-dim` on the sticky header | 5.74:1 |
| Card text and SVG labels `--ink-dim` on `--bg-card` | 5.34:1 |
| Code comments `--ink-dim` on code background | 6.13:1 |
| Links `--copper-hi` on page background | 10.11:1 |
| Links `--copper-hi` on `--bg-card` | 9.24:1 |
| Kicker `--copper` on page background | 6.94:1 |
| Primary button text on its gradient (darkest end, `--copper`) | 6.67:1 |
| Primary button text on hover | 7.55:1 |
| Ghost button border `--copper-lo` on page background | 3.72:1 |
| Focus outline `--copper-hi` on page, header and card backgrounds | 9.24:1 or more |
| `--copper-hi` links against surrounding `--ink-dim` text | 1.73:1 (fails 3:1, so these links must stay underlined) |

When you change a colour token, recompute these pairs.

## Motion

| Check | Expected result |
| --- | --- |
| Default motion setting | The wires in the hero illustration animate and in-page links scroll smoothly. |
| Operating system or browser set to reduce motion (`prefers-reduced-motion: reduce`) | The wires are static and in-page links jump without smooth scrolling. |

## Layout

| Check | Expected result |
| --- | --- |
| Desktop width | The header is a single row, the hero shows the text and illustration side by side, and the feature cards form a three-column grid. Only the code blocks scroll sideways. |
| Narrow width | The header wraps onto several lines without horizontal page scrolling. The skip link and focus outlines stay visible. |
