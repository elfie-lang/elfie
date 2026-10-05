# Next.js 15: request APIs are asynchronous

From Next.js 15, the APIs that read a request are asynchronous and must be awaited. Code written for
Next.js 13 or 14 that reads them synchronously still runs with a warning in development, but it is
deprecated and must not be generated.

- `params` and `searchParams`, the props of a page, layout, or route handler, are Promises:
  `export default async function Page({ params }: { params: Promise<{ id: string }> }) { const { id } = await params; }`
- In a route handler the second argument holds the same Promise: `export async function GET(request: Request, { params }: { params: Promise<{ id: string }> })`.
- `cookies()`, `headers()`, and `draftMode()` from `next/headers` return Promises: `const store = await cookies();`.
- `fetch` requests and `GET` route handlers are no longer cached by default; opt in with `cache: "force-cache"` or `export const dynamic = "force-static"`.
- A client component cannot be async; unwrap a Promise prop there with `React.use(params)`.
