# work-match – UI Style Guide

> Inflatable sticker universe on pastel paper, scaled down to a desktop app window.
> Based on the "Slush" style reference. The one exception: the match popup stays on
> **liquid glass**.

Tokens live in `src/style.css` (`:root`). Use the tokens, not raw hex values.

## Feel

- A sticker book: pastel paper canvas, black hand-cut outlines, a rainbow of flat sticker colors.
- Display type is chunky and crushed so words read like objects, not sentences.
- Everything is soft: pill buttons, 20px+ card corners.
- Elevation comes from color and 1px black outlines, never shadows or gradients.

## Colors

| Token | Value | Use |
|---|---|---|
| `--color-carbon` | `#000000` | Text, every outline, filled primary button |
| `--color-paper-white` | `#ffffff` | Cards, inputs, outlined buttons |
| `--color-sky-wash` | `#dceeff` | Window background (main surface) |
| `--color-concrete-gray` | `#cccccc` | Secondary bands, dividers |
| `--color-soft-mist` | `#e9e9e9` | Disabled states, subtle fills |
| `--color-electric-blue` | `#4da2ff` | Brand / decoration only, **never** a button or link color |
| `--color-mint-pop` | `#55db9c` | Sticker accent, **not** a success color |
| `--color-lavender` | `#e9ccff` | Soft card / tag fills, avatar |
| `--color-ember` | `#fb4903` | Hot sticker accent; also used for error text outlines |
| `--color-sunburst` | `#ffd731` | Sticker fills; never behind body text |
| `--color-voltage-violet` | `#5c4ade` | Deep accent stickers / cards |

Use several sticker colors per screen. Don't pick one as "the" accent.

## Type

| Role | Font | Size / line-height | Notes |
|---|---|---|---|
| Display (wordmark, big headings) | `--font-display`: Bowlby One (stand-in for Lateral 800) | 56–96px / 0.78 | Uppercase, crushed leading. Never body text |
| Heading | `--font-ui` 700 | 24–30px / 1.1 | Names, section titles |
| Body | `--font-ui` 500 | 13–15px / 1.4, -0.01em | |
| Labels / buttons | `--font-ui` 700 | 12–13px, +0.032em, uppercase | Pill controls and field labels |
| Caption | `--font-ui` 500 | 12px / 1.56 | Status lines, fine print |

`--font-ui` is Inter (stand-in for Aeonik Pro). Both fonts are bundled from `@fontsource`, so they work offline.

## Shape & spacing

- **4px base unit.** Gaps 4–12px inside components, 16–24px between them, 24px card padding.
- **Radius:**
  - buttons, pills, tags and toggles: `--radius-pill` (fully round)
  - cards: `--radius-card` (20px)
  - big panels: `--radius-card-lg` (40px)
  - inputs: 16px
- **Outlines:** every interactive element and card has `1px solid var(--color-carbon)`.
- No `box-shadow`, no gradients, ever.

## Components

| Component | Spec |
|---|---|
| **Primary button** (`button.primary`) | Black fill, white text, pill, 700 uppercase 12px +0.032em, 10px/16px padding. One per screen |
| **Ghost button** (`button.secondary`) | White fill, black 1px outline, black text, pill. Secondary actions and task buttons |
| **Link button** (`button.link`) | No fill or border, black 700 text, underline on hover. Log out, Quit, Stop, Cancel |
| **Card** (`.card`, `.details`) | White, 1px black outline, 20px radius, no shadow |
| **Sticker** (`.sticker`) | Small flat shape or emoji badge with a sticker-color fill, 1px black outline, 16–20px radius, slightly rotated. Decoration only, not clickable |
| **Avatar** | Circle in a sticker color with a 1px black outline, initial in display font |
| **Input / select** | White, 1px black outline, 16px radius; focus = 2px black outline |
| **Status line** | Caption text. Neutral = black on white pill; error = ember outline |

## Liquid glass exception: the match popup

The match popup (`match.html`, `src/match.css`) stays on native **clear liquid glass** with a light tint (`Some((0, 0, 0, 70))` in `src-tauri/src/lib.rs`), white text and a soft text shadow. It borrows from this guide:
- the same fonts
- the display font for the eyebrow
- pill buttons:
  - "Work together" is the primary: white fill, black text
  - "Not now" is a ghost: transparent with a white 1px outline
- the 20px-radius person card

It doesn't use the pastel canvas or black outlines, because they would hide the glass.

## Do / Don't

- **Do** outline everything in black, round everything, and scatter 2–4 sticker colors per screen.
- **Do** pair display type with a sticker or two, so it never sits alone.
- **Don't** use blue as an action color, green as "success", or yellow behind text.
- **Don't** add shadows, gradients or radii under 16px on cards.
- **Don't** add motion beyond small hover changes.
