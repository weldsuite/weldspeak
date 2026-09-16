import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  // Turbopack is the default bundler in Next.js 16 for both `next dev` and
  // `next build`. Scripts also pass `--turbopack` explicitly.
  async redirects() {
    return [
      {
        source: "/download",
        destination: "/desktop",
        permanent: false,
      },
    ];
  },
};

export default nextConfig;
