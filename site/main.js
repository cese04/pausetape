// Pausetape's website: sections that fade in, the step-by-step tour, and download links filled
// in from the latest GitHub release. The page works without any of it.

const REPO = "cese04/pausetape";
const still = matchMedia("(prefers-reduced-motion: reduce)").matches;

// Sections fade up the first time they come into view.
function reveal() {
  const parts = document.querySelectorAll("[data-reveal]");
  if (still || !("IntersectionObserver" in window)) {
    parts.forEach((p) => p.classList.add("is-in"));
    return;
  }
  const io = new IntersectionObserver((seen) => {
    for (const s of seen) {
      if (s.isIntersecting) {
        s.target.classList.add("is-in");
        io.unobserve(s.target);
      }
    }
  }, { rootMargin: "0px 0px -10% 0px" });
  parts.forEach((p) => io.observe(p));
}

// The tour: each step is a screenshot with one or more marks, boxes around what to look at,
// which the ring moves between while the note under the screen explains each. Marks are
// "left top width height" in percent of the screenshot, separated by "|" (see docs/WEBSITE.md).
function tour(root) {
  const steps = [...root.querySelectorAll(".tour-steps > li")];
  const shots = [...root.querySelectorAll(".tour-shot")];
  const mark = root.querySelector(".mark");
  const note = root.querySelector(".tour-note");
  const tape = root.querySelector(".tour-tape span");
  const play = root.querySelector("[data-tour-play]");
  const plan = steps.map((li) => ({
    marks: li.dataset.marks.split("|").map((m) => m.trim().split(/\s+/).map(Number)),
    notes: li.dataset.notes.split("|").map((n) => n.trim()),
  }));
  const BEAT = 3600; // ms per mark
  let step = 0, beat = 0, playing = !still, visible = false, timer = 0, started = 0;

  function show(s, b) {
    const changed = s !== step || !mark.classList.contains("is-on");
    step = s;
    beat = b;
    steps.forEach((li, i) => li.setAttribute("aria-current", i === s ? "step" : "false"));
    shots.forEach((p, i) => p.classList.toggle("is-active", i === s));
    const [x, y, w, h] = plan[s].marks[b];
    // A little air around what's marked.
    Object.assign(mark.style, { left: `calc(${x}% - 6px)`, top: `calc(${y}% - 6px)`, width: `calc(${w}% + 12px)`, height: `calc(${h}% + 12px)` });
    mark.classList.add("is-on");
    zoom(x, y, w, h);
    const text = plan[s].notes[b] || "";
    if (changed || still) {
      note.textContent = text;
    } else {
      note.classList.add("is-changing");
      setTimeout(() => { note.textContent = text; note.classList.remove("is-changing"); }, 250);
    }
    // Lazy images: make sure the next step's picture is on its way.
    shots[(s + 1) % shots.length].querySelector("img").loading = "eager";
    run();
  }

  // On a narrow screen, close in on the mark: as near as twice the size, as long as the mark
  // still fits, and never past the screenshot's edges.
  const zoomer = root.querySelector(".tour-zoom");
  const narrow = matchMedia("(max-width: 700px)");
  function zoom(x, y, w, h) {
    const z = narrow.matches ? Math.max(1, Math.min(2.2, 86 / (w + 4), 86 / (h + 4))) : 1;
    const clamp = (v) => Math.min(0, Math.max(100 - 100 * z, v));
    const tx = clamp(50 - z * (x + w / 2)), ty = clamp(50 - z * (y + h / 2));
    zoomer.style.transform = `translate(${tx}%, ${ty}%) scale(${z})`;
    zoomer.style.setProperty("--zoom", z);
  }
  narrow.addEventListener("change", () => show(step, beat));

  // The tape under the screen runs through the whole tour, as a progress bar.
  function progress() {
    const total = plan.reduce((n, p) => n + p.marks.length, 0);
    const before = plan.slice(0, step).reduce((n, p) => n + p.marks.length, 0) + beat;
    const within = playing ? Math.min((performance.now() - started) / BEAT, 1) : 0;
    tape.style.width = `${((before + within) / total) * 100}%`;
    if (playing && visible) requestAnimationFrame(progress);
  }

  function run() {
    clearTimeout(timer);
    started = performance.now();
    if (!playing || !visible) { progress(); return; }
    requestAnimationFrame(progress);
    timer = setTimeout(next, BEAT);
  }

  function next() {
    const p = plan[step];
    if (beat + 1 < p.marks.length) show(step, beat + 1);
    else show((step + 1) % plan.length, 0);
  }

  function back() {
    if (beat > 0) show(step, beat - 1);
    else show((step - 1 + plan.length) % plan.length, 0);
  }

  function setPlaying(on) {
    playing = on;
    play.classList.toggle("is-paused", !on);
    play.setAttribute("aria-label", on ? "Pause the tour" : "Play the tour");
    run();
  }

  steps.forEach((li, i) => li.querySelector("button").addEventListener("click", () => { setPlaying(false); show(i, 0); }));
  root.querySelector("[data-tour-next]").addEventListener("click", () => { setPlaying(false); next(); });
  root.querySelector("[data-tour-prev]").addEventListener("click", () => { setPlaying(false); back(); });
  play.addEventListener("click", () => setPlaying(!playing));

  // Plays only while on screen, so it starts at the beginning when someone gets there.
  if ("IntersectionObserver" in window) {
    new IntersectionObserver(([e]) => { visible = e.isIntersecting; run(); }, { threshold: 0.4 }).observe(root);
  } else {
    visible = true;
  }
  // ?step=3.2 opens on the third step's second mark, paused: for checking the marks after new
  // screenshots (docs/WEBSITE.md).
  const asked = new URLSearchParams(location.search).get("step")?.split(".").map((n) => Number(n) - 1);
  if (asked && plan[asked[0]]?.marks[asked[1] || 0]) {
    setPlaying(false);
    show(asked[0], asked[1] || 0);
    return;
  }
  setPlaying(playing);
  show(0, 0);
}

// Which computer this is, to point at the right download.
function platform() {
  const p = (navigator.userAgentData?.platform || navigator.platform || "").toLowerCase();
  const ua = navigator.userAgent.toLowerCase();
  if (/iphone|ipad|android/.test(ua)) return null;
  if (p.includes("mac")) return "mac";
  if (p.includes("win")) return "windows";
  if (p.includes("linux")) return "linux";
  return null;
}

const NAMES = { mac: "macOS", windows: "Windows", linux: "Linux" };

async function downloads() {
  const mine = platform();
  const hero = document.querySelector("[data-download-hero]");
  if (mine) {
    document.querySelector(`.platform[data-os="${mine}"]`)?.classList.add("is-yours");
    hero.textContent = `Download for ${NAMES[mine]}`;
  }
  let release;
  try {
    const r = await fetch(`https://api.github.com/repos/${REPO}/releases/latest`, { headers: { Accept: "application/vnd.github+json" } });
    if (!r.ok) return; // No release yet: the page already says so.
    release = await r.json();
  } catch {
    return;
  }
  const status = document.querySelector("[data-release-status]");
  const date = new Date(release.published_at).toLocaleDateString("en", { year: "numeric", month: "long", day: "numeric" });
  status.innerHTML = "";
  status.append(`Version ${release.tag_name.replace(/^v/, "")}, from ${date}. `);
  const notes = Object.assign(document.createElement("a"), { href: release.html_url, textContent: "What's new" });
  status.append(notes, ".");
  for (const a of document.querySelectorAll("[data-asset]")) {
    const asset = release.assets.find((x) => new RegExp(a.dataset.asset).test(x.name));
    if (!asset) continue;
    a.href = asset.browser_download_url;
    a.textContent = `Download · ${Math.round(asset.size / 1e6)} MB`;
  }
  const own = mine && document.querySelector(`.platform[data-os="${mine}"] [data-asset]`);
  if (own && own.href.includes("/download/")) hero.href = own.href;
}

reveal();
document.querySelectorAll("[data-tour]").forEach(tour);
downloads();
