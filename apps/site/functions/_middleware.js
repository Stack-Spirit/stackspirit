/**
 * Cloudflare Pages middleware for https://stackspirit.io.
 *
 * The apex host is canonical. Older hosts keep their path and query string but
 * answer a permanent 301 to the apex, so search results and shared links keep
 * working:
 *
 *   https://www.stackspirit.io/quickstart?x=1
 *     -> https://stackspirit.io/quickstart?x=1
 *   https://stackspirit-site.pages.dev/
 *     -> https://stackspirit.io/
 *
 * Preview deployment hostnames such as <hash>.stackspirit-site.pages.dev are
 * deliberately not redirected, so reviewing a preview never bounces to
 * production. The API, relay and app subdomains are unaffected.
 */

const CANONICAL_ORIGIN = "https://stackspirit.io";

const REDIRECT_HOSTS = new Set([
  "www.stackspirit.io",
  "stackspirit-site.pages.dev",
]);

export async function onRequest(context) {
  const url = new URL(context.request.url);
  // A trailing root-label dot is still the same host.
  const hostname = url.hostname.toLowerCase().replace(/\.$/, "");
  if (!REDIRECT_HOSTS.has(hostname)) {
    return context.next();
  }

  // Assign the path and query instead of resolving a relative URL: a path
  // that begins with "//" must never be interpreted as another host.
  const target = new URL(CANONICAL_ORIGIN);
  target.pathname = url.pathname;
  target.search = url.search;
  return Response.redirect(target.toString(), 301);
}
