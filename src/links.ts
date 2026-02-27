export interface LinkEntry {
  label: string;
  url: string;
  scrambleDuration?: number;
}

export const links: LinkEntry[] = [
  { label: "GitHub", url: "https://github.com/lukepearce" },
  { label: "Claude Code", url: "https://claude.ai" },
  { label: "Are.na", url: "https://www.are.na/luke-pearce" },
  { label: "Letterboxd", url: "https://letterboxd.com" },
  { label: "Email", url: "mailto:hello@lukepearce.co.uk" },
];
