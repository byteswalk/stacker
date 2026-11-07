// The faint drawing behind a home page card: flat 2D line art in the card's own colour.
const STROKE = { fill: "none", stroke: "currentColor", strokeWidth: 1.6, strokeLinecap: "round", strokeLinejoin: "round" } as const;
const TINT = { fill: "currentColor", fillOpacity: .14 };

const ART = {
  sessions: <>
    <path d="M18 12h60a10 10 0 0 1 10 10v30a10 10 0 0 1-10 10H40L24 76V62h-6A10 10 0 0 1 8 52V22a10 10 0 0 1 10-10z" {...TINT} />
    <path d="M22 28h52M22 40h36" />
    <path d="M56 70h46a10 10 0 0 1 10 10v22a10 10 0 0 1-10 10h-4v12l-14-12H56a10 10 0 0 1-10-10V80a10 10 0 0 1 10-10z" {...TINT} />
    <path d="M60 86h38M60 96h24" />
  </>,
  proxy: <>
    <circle cx="60" cy="60" r="48" {...TINT} />
    <ellipse cx="60" cy="60" rx="22" ry="48" />
    <path d="M12 60h96M20 34h80M20 86h80M60 12v96" />
  </>,
  disk: <>
    <rect x="14" y="4" width="92" height="112" rx="8" {...TINT} />
    <circle cx="60" cy="48" r="36" {...TINT} />
    <circle cx="60" cy="48" r="26" /><circle cx="60" cy="48" r="15" /><circle cx="60" cy="48" r="5" fill="currentColor" />
    <circle cx="93" cy="100" r="7" />
    <path d="M90 94L66 54" strokeWidth="4" />
    <circle cx="22" cy="12" r="2" /><circle cx="98" cy="12" r="2" /><circle cx="22" cy="108" r="2" />
    <path d="M26 100h34M26 106h22" />
  </>,
  vault: <>
    <path d="M24 104v8h14v-8M82 104v8h14v-8" />
    <rect x="10" y="6" width="100" height="98" rx="8" {...TINT} />
    <rect x="20" y="16" width="80" height="78" rx="5" />
    <path d="M6 28v14M6 68v14" strokeWidth="4" />
    <circle cx="60" cy="55" r="22" {...TINT} />
    <circle cx="60" cy="55" r="13" />
    <path d="M60 33v9M60 68v9M38 55h9M73 55h9M44.500 39.500l6 6M69.500 64.500l6 6M75.500 39.500l-6 6M50.500 64.500l-6 6" />
  </>,
};

export function CoreArt({ name }: { name: keyof typeof ART }) {
  return <svg className={"core-art " + name} viewBox="0 0 120 124" aria-hidden="true" {...STROKE}>{ART[name]}</svg>;
}
