export function PageIntro({
  label,
  title,
  description,
}: {
  label: string;
  title: string;
  description: string;
}) {
  return (
    <section className="container py-16 md:py-24">
      <p className="text-[10px] font-semibold tracking-[1.3px] text-muted-foreground uppercase">
        {label}
      </p>
      <h1 className="mt-3 max-w-3xl font-display text-4xl leading-[0.98] font-semibold tracking-[-0.05em] text-pretty lg:text-[4.5rem]">
        {title}
      </h1>
      <p className="mt-5 max-w-lg text-[14px] leading-relaxed text-muted-foreground">
        {description}
      </p>
    </section>
  );
}
