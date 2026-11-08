import type { BaseLayoutProps } from "fumadocs-ui/layouts/shared";
import { asset, repo } from "./site";

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <span className="flex items-center gap-2 font-semibold tracking-tight">
          {/* eslint-disable-next-line @next/next/no-img-element -- static export, no image optimisation */}
          <img src={asset("/icon.svg")} alt="" width={22} height={22} />
          Granary
        </span>
      ),
      url: "/",
    },
    githubUrl: repo,
    links: [{ text: "Docs", url: "/docs", active: "nested-url" }],
  };
}
