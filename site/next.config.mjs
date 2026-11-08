import { createMDX } from "fumadocs-mdx/next";

/** GitHub Pages project URL: https://<owner>.github.io/<repo>/ (overridable for a custom domain). */
const basePath = process.env.SITE_BASE_PATH ?? "/granary-desktop";

/** @type {import('next').NextConfig} */
const config = {
  output: "export",
  basePath,
  trailingSlash: true,
  images: { unoptimized: true },
  reactStrictMode: true,
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
};

export default createMDX()(config);
