// Small inline icons (MingCute-style outlines, 24px grid, drawn with currentColor).

type P = { size?: number };
const svg = (size: number, children: React.ReactNode) => (
  <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.8} strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
    {children}
  </svg>
);

export const SearchIcon = ({ size = 16 }: P) => svg(size, <><circle cx="11" cy="11" r="6.5" /><path d="m20 20-4.2-4.2" /></>);
export const CloseIcon = ({ size = 16 }: P) => svg(size, <path d="M6 6l12 12M18 6 6 18" />);
export const AddIcon = ({ size = 16 }: P) => svg(size, <path d="M12 5v14M5 12h14" />);
export const FolderIcon = ({ size = 16 }: P) => svg(size, <path d="M3.5 7.5a2 2 0 0 1 2-2h4l2 2h7a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2h-13a2 2 0 0 1-2-2z" />);
export const SparkIcon = ({ size = 16 }: P) => svg(size, <path d="M12 3v4M12 17v4M3 12h4M17 12h4M6 6l2.5 2.5M15.5 15.5 18 18M6 18l2.5-2.5M15.5 8.5 18 6" />);
export const ExternalIcon = ({ size = 14 }: P) => svg(size, <path d="M14 5h5v5M19 5l-8 8M18 14v4a1 1 0 0 1-1 1H6a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h4" />);
export const ChevronDownIcon = ({ size = 12 }: P) => svg(size, <path d="m6 9 6 6 6-6" />);

export function CaptionIcon({ kind }: { kind: "minimize" | "maximize" | "close" }) {
  return (
    <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden="true" stroke="currentColor" strokeWidth="1" fill="none">
      {kind === "minimize" && <path d="M0 5.5h10" />}
      {kind === "maximize" && <rect x="0.5" y="0.5" width="9" height="9" rx="1" />}
      {kind === "close" && <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />}
    </svg>
  );
}
