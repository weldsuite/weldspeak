"use client";

import { cn } from "cn";
import { Avatar, AvatarImage } from "@/components/ui/avatar";
import { Card } from "@/components/ui/card";

interface TestimonialBasicGridItem {
  id?: string;
  name: string;
  avatar: string;
  content: string;
  role?: string;
  username?: string;
  date?: string;
  link?: string;
  icon?: string;
}

interface TestimonialBasicGridProps {
  heading: string;
  description: string;
  testimonials: TestimonialBasicGridItem[];
  className?: string;
}

interface Testimonial9Props extends TestimonialBasicGridProps {}
type Props = Partial<Testimonial9Props>;

const defaultProps: Testimonial9Props = {
  heading: "Testimonials",
  description:
    "Hear from our happy clients about their experience with our products and services.",
  testimonials: [
    {
      id: "1",
      name: "Sarah Chen",
      username: "sarahchen",
      date: "2024-02-12",
      role: "CEO & Founder",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar3.jpg",
      content:
        "This component library has completely transformed how we build products. We shipped our entire customer-facing dashboard in half the time it would have taken with our old workflow, and the design consistency across every page was noticeably better.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/x-icon.svg",
    },
    {
      id: "2",
      name: "Marcus Rodriguez",
      username: "marcusr",
      date: "2024-02-10",
      role: "CTO",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar7.jpg",
      content:
        "The attention to accessibility and performance is outstanding. Our Lighthouse scores improved by 15 points across the board after we migrated, and we passed our first external accessibility audit with zero critical issues. I was skeptical at first, but the quality of the underlying markup convinced me within a week.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/linkedin-icon.svg",
    },
    {
      id: "3",
      name: "Emily Watson",
      username: "emilywatson",
      date: "2024-02-08",
      role: "Head of Product",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar12.jpg",
      content:
        "Finally, a design system that developers actually want to use. The documentation is clear, the components are flexible, and the defaults are sensible.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/x-icon.svg",
    },
    {
      id: "4",
      name: "David Kim",
      username: "davidkim",
      date: "2024-02-05",
      role: "Tech Lead",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar18.jpg",
      content:
        "We evaluated five UI libraries over two quarters before settling on this one. What set it apart was the balance between opinionated defaults and customization flexibility. Every component felt production-ready out of the box.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/instagram-icon.svg",
    },
    {
      id: "5",
      name: "Rachel Foster",
      username: "rachelf",
      date: "2024-02-01",
      role: "Senior Designer",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar22.jpg",
      content:
        "As a designer, I appreciate how closely the components match our Figma designs. The design-to-dev handoff has never been smoother. Every pixel is intentional, and the spacing system makes it easy to maintain visual consistency across dozens of screens without constant back-and-forth.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/facebook-icon.svg",
    },
    {
      id: "6",
      name: "James Mitchell",
      username: "jamesm",
      date: "2024-01-28",
      role: "Full Stack Developer",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar25.jpg",
      content:
        "The TypeScript support is best-in-class. Autocomplete just works, prop types catch mistakes before they hit production, and the DX is genuinely enjoyable. I used to dread building forms and data tables — now they take a fraction of the time. The generics on the table component alone are worth the switch.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/x-icon.svg",
    },
    {
      id: "7",
      name: "Nina Patel",
      username: "ninapatel",
      date: "2024-01-22",
      role: "UX Engineer",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar8.jpg",
      content:
        "These components handle edge cases I didn't even think to test for. Dark mode, RTL support, keyboard navigation, screen reader announcements — it's all built in from the start. We used to spend the last two weeks of every release cycle patching accessibility bugs. That just doesn't happen anymore.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/linkedin-icon.svg",
    },
    {
      id: "8",
      name: "Alex Thompson",
      username: "alext",
      date: "2024-01-18",
      role: "Engineering Manager",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar15.jpg",
      content:
        "Our team's velocity increased measurably after adopting this library. Less time on UI boilerplate means more time on the features our customers actually care about.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/x-icon.svg",
    },
    {
      id: "9",
      name: "Henry Garcia",
      username: "henryg",
      date: "2023-08-25",
      role: "Product Lead",
      avatar: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/modern/avatars/avatar20.jpg",
      content:
        "We rebuilt our entire onboarding flow using these blocks and launched it in under three weeks. The animation primitives gave us polished micro-interactions without pulling in a separate motion library, and the responsive behavior just worked on every breakpoint we tested. Our activation rate is up 20% since the redesign, and I'm confident the UI quality played a big part in that.",
      link: "#",
      icon: "https://deifkwefumgah.cloudfront.net/shadcnblocks/image-set/placeholder/testimonials/social-network-icons/instagram-icon.svg",
    },
  ],
};

const Testimonial9 = (props: Props) => {
  const { heading, description, testimonials, className } = {
    ...defaultProps,
    ...props,
  };

  const list = testimonials.slice(0, 6);

  return (
    <section className={cn("py-32", className)}>
      <div className="container mx-auto">
        <div className="flex flex-col items-center gap-6">
          <h2 className="text-center text-3xl font-semibold lg:text-5xl">
            {heading}
          </h2>
          <p className="text-muted-foreground lg:text-lg">{description}</p>
        </div>
        <div className="mt-14 w-full">
          <div className="columns-1 gap-5 md:columns-2 lg:columns-3">
            {list.map((testimonial, idx) => {
              return (
                <Card key={idx} className="mb-5 break-inside-avoid p-5">
                  <div className="flex justify-between">
                    <div className="flex gap-4 leading-5">
                      <Avatar className="size-9 rounded-full ring-1 ring-input">
                        <AvatarImage
                          src={testimonial.avatar}
                          alt={testimonial.name}
                        />
                      </Avatar>
                      <div className="text-sm">
                        <p className="font-medium">{testimonial.name}</p>
                        <p className="text-muted-foreground">
                          {testimonial.role}
                        </p>
                      </div>
                    </div>
                    {testimonial.icon ? (
                      <a href={testimonial.link ?? "#"}>
                        <img
                          alt="Testimonial source"
                          src={testimonial.icon}
                          className="size-4 dark:invert"
                        />
                      </a>
                    ) : null}
                  </div>
                  <div className="mt-2 leading-7 text-muted-foreground">
                    <q>{testimonial.content}</q>
                  </div>
                </Card>
              );
            })}
          </div>
        </div>
      </div>
    </section>
  );
};

export { Testimonial9 };
