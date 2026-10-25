// Feeds the mouse position to src/effects.css: the cursor follower (a dot on
// the pointer and a ring that trails it on a spring) and the spotlight on the
// hovered card (--x / --y on that card).

const CARDS = ".details, .card, .friend";
const HOVERABLE = "button, a, input, select, label, .editable, .details, .card, .friend";
// Fraction of the remaining distance the ring covers each frame.
const RING_EASE = 0.18;

export function startEffects() {
  const glow = document.createElement("div");
  glow.className = "glow";
  glow.setAttribute("aria-hidden", "true");
  glow.append(document.createElement("span"), document.createElement("span"), document.createElement("span"));
  document.body.prepend(glow);

  const dot = document.createElement("div");
  dot.className = "cursor-dot";
  const ring = document.createElement("div");
  ring.className = "cursor-ring";
  document.body.append(ring, dot);
  document.body.classList.add("mouse-out");

  let frame = 0;
  let last = null;
  const pointer = { x: -100, y: -100 };
  const trail = { x: -100, y: -100 };
  let following = false;

  // The ring eases toward the pointer every frame and stops once it's there.
  function follow() {
    trail.x += (pointer.x - trail.x) * RING_EASE;
    trail.y += (pointer.y - trail.y) * RING_EASE;
    ring.style.transform = `translate(${trail.x}px, ${trail.y}px)`;
    following = Math.abs(pointer.x - trail.x) + Math.abs(pointer.y - trail.y) > 0.3;
    if (following) requestAnimationFrame(follow);
  }

  document.addEventListener("pointerdown", () => document.body.classList.add("cursor-down"));
  document.addEventListener("pointerup", () => document.body.classList.remove("cursor-down"));

  document.addEventListener("pointermove", (event) => {
    last = event;
    pointer.x = event.clientX;
    pointer.y = event.clientY;
    if (document.body.classList.contains("mouse-out")) {
      // Coming back in: start the ring on the pointer instead of flying in.
      trail.x = pointer.x;
      trail.y = pointer.y;
      document.body.classList.remove("mouse-out");
    }
    dot.style.transform = `translate(${pointer.x}px, ${pointer.y}px)`;
    if (!following) {
      following = true;
      requestAnimationFrame(follow);
    }
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      const target = last.target instanceof Element ? last.target : null;
      document.body.classList.toggle("cursor-hover", Boolean(target?.closest(HOVERABLE)));
      const card = last.target instanceof Element ? last.target.closest(CARDS) : null;
      if (card) {
        const box = card.getBoundingClientRect();
        card.style.setProperty("--x", `${last.clientX - box.left}px`);
        card.style.setProperty("--y", `${last.clientY - box.top}px`);
      }
    });
  });

  document.documentElement.addEventListener("pointerleave", () => document.body.classList.add("mouse-out"));
}
