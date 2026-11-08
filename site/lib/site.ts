/** Static export under a GitHub Pages project path: plain <img>/<a> need the base path themselves. */
export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
export const asset = (path: string) => `${basePath}${path}`;

export const repo = "https://github.com/mrghxst/granary-desktop";
export const releases = `${repo}/releases`;
export const blob = (path: string) => `${repo}/blob/main/${path}`;
