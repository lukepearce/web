export interface LinkEntry {
  label: string;
  url: string;
  scrambleDuration?: number;
}

export const links: LinkEntry[] = [
  { label: "GitHub", url: "https://github.com/lukepearce" },
  { label: "Instagram", url: "https://www.instagram.com/_luke_pearce" },
  { label: "Letterboxd", url: "https://letterboxd.com/lukepearce" },
  { label: "Email", url: "mailto:hello@lukepearce.co.uk" },
  { label: "Strava", url: "https://www.strava.com/athletes/983545"}
];
