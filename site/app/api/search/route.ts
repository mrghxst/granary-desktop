import { createFromSource } from "fumadocs-core/search/server";
import { source } from "@/lib/source";

// Exported once at build time; the browser downloads the index and searches locally (GitHub Pages is static).
export const revalidate = false;
export const { staticGET: GET } = createFromSource(source);
