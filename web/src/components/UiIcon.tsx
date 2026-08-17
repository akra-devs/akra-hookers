type IconName =
  | "activity"
  | "brand"
  | "close"
  | "expand"
  | "folder"
  | "inbox"
  | "location"
  | "plus"
  | "settings"
  | "terminal"
  | "trash";

type UiIconProps = {
  name: IconName;
  size?: number;
  className?: string;
};

export function UiIcon({ name, size = 18, className }: UiIconProps) {
  const common = {
    width: size,
    height: size,
    viewBox: "0 0 24 24",
    fill: "none",
    stroke: "currentColor",
    strokeWidth: 1.7,
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    className,
    "aria-hidden": true,
  };

  switch (name) {
    case "brand":
      return <svg {...common}><path d="M8 4 3 12l5 8M16 4l5 8-5 8M14 3l-4 18" /></svg>;
    case "close":
      return <svg {...common}><path d="m6 6 12 12M18 6 6 18" /></svg>;
    case "expand":
      return <svg {...common}><path d="M8 3H3v5M16 3h5v5M8 21H3v-5m18 0v5h-5M3 8l6-5m6 0 6 5M3 16l6 5m6 0 6-5" /></svg>;
    case "activity":
      return <svg {...common}><circle cx="5" cy="12" r="2" /><circle cx="12" cy="5" r="2" /><circle cx="19" cy="10" r="2" /><circle cx="13" cy="19" r="2" /><path d="m6.6 10.8 3.8-4.5m3.5-.7 3.3 3.1m.1 3.1-3 5.3m-3.2.8-4.4-4.6" /></svg>;
    case "inbox":
      return <svg {...common}><path d="M4 5h16l2 9v5H2v-5l2-9Z" /><path d="M2 14h5l2 3h6l2-3h5" /></svg>;
    case "folder":
      return <svg {...common}><path d="M3 6.5h7l2 2h9v10H3v-12Z" /></svg>;
    case "location":
      return <svg {...common}><path d="M20 10c0 5-8 11-8 11S4 15 4 10a8 8 0 1 1 16 0Z" /><circle cx="12" cy="10" r="2.5" /></svg>;
    case "plus":
      return <svg {...common}><path d="M12 5v14M5 12h14" /></svg>;
    case "settings":
      return <svg {...common}><circle cx="12" cy="12" r="3" /><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.9l.1.1-2.8 2.8-.1-.1a1.7 1.7 0 0 0-1.9-.3 1.7 1.7 0 0 0-1 1.6v.2h-4V21a1.7 1.7 0 0 0-1-1.6 1.7 1.7 0 0 0-1.9.3l-.1.1L4.2 17l.1-.1a1.7 1.7 0 0 0 .3-1.9A1.7 1.7 0 0 0 3 14H2.8v-4H3a1.7 1.7 0 0 0 1.6-1 1.7 1.7 0 0 0-.3-1.9L4.2 7 7 4.2l.1.1a1.7 1.7 0 0 0 1.9.3A1.7 1.7 0 0 0 10 3V2.8h4V3a1.7 1.7 0 0 0 1 1.6 1.7 1.7 0 0 0 1.9-.3l.1-.1L19.8 7l-.1.1a1.7 1.7 0 0 0-.3 1.9 1.7 1.7 0 0 0 1.6 1h.2v4H21a1.7 1.7 0 0 0-1.6 1Z" /></svg>;
    case "terminal":
      return <svg {...common}><rect x="3" y="4" width="18" height="16" rx="2" /><path d="m7 9 3 3-3 3m6 0h4" /></svg>;
    case "trash":
      return <svg {...common}><path d="M4 7h16M9 7V4h6v3m3 0-1 13H7L6 7m4 4v5m4-5v5" /></svg>;
  }
}
