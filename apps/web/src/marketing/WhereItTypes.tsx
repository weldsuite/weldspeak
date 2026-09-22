import {
  AppWindow,
  Globe,
  Mail,
  MessageSquare,
  StickyNote,
  Terminal,
} from "lucide-react";

const places = [
  { icon: MessageSquare, label: "Slack", tone: "bg-[#F8C4B4] text-[#9A3B28]" },
  { icon: Mail, label: "Email", tone: "bg-[#F5D0A8] text-[#B45A1A]" },
  { icon: Terminal, label: "Your IDE", tone: "bg-[#D7C8F0] text-[#5B3F8A]" },
  { icon: Globe, label: "The browser", tone: "bg-[#C8E8D4] text-[#2F6B48]" },
  { icon: StickyNote, label: "Notepad", tone: "bg-[#F6E4B8] text-[#8A5A12]" },
  { icon: AppWindow, label: "Any focused field", tone: "bg-[#C8D8F4] text-[#2F4A8A]" },
];

export function WhereItTypes() {
  return (
    <section className="border-t border-border py-10">
      <div className="container flex flex-col items-start justify-between gap-8 md:flex-row md:items-center">
        <p className="max-w-[10rem] text-[13px] leading-snug font-medium">
          A whole day’s work.
          <br />
          One shared glossary.
        </p>
        <ul className="flex flex-wrap items-center gap-3">
          {places.map((place) => (
            <li key={place.label} className="flex flex-col items-center gap-2">
              <span
                className={`flex size-11 items-center justify-center rounded-2xl ${place.tone}`}
              >
                <place.icon className="size-4" strokeWidth={1.75} />
              </span>
              <span className="text-[11px] text-muted-foreground">{place.label}</span>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
