import { component$, useContext } from "@builder.io/qwik";
import { Link, useLocation } from "@builder.io/qwik-city";
import { GuestContext } from "~/state/guest";
import { ThemeToggle } from "./theme-toggle";

const NAV = [
  { href: "/trade/", label: "Trade" },
  { href: "/operate/", label: "Operate" },
  { href: "/engine/", label: "Engine" },
  { href: "/load/", label: "Load" },
];

/** True on the app pages that get the full navigation. */
export function isAppPath(pathname: string): boolean {
  return NAV.some((n) => pathname.startsWith(n.href.slice(0, -1)));
}

export const TopBar = component$(() => {
  const loc = useLocation();
  const guest = useContext(GuestContext);
  const path = loc.url.pathname;
  const app = isAppPath(path);

  return (
    <header class="topbar">
      <div class="wrap topbar-inner">
        <Link href="/" class="brand">
          <span class="full">Thande Stonks Exchange</span>
          <span class="short" aria-hidden="true">
            TSE
          </span>
        </Link>
        {app && (
          <nav class="mainnav" aria-label="Main">
            {NAV.map((n) => (
              <Link
                key={n.href}
                href={n.href}
                aria-current={
                  path.startsWith(n.href.slice(0, -1)) ? "page" : undefined
                }
              >
                {n.label}
              </Link>
            ))}
          </nav>
        )}
        <div class="topbar-end">
          {guest.status === "joined" && (
            <span class="guest" title={guest.displayName}>
              Guest <strong>{guest.displayName}</strong>
            </span>
          )}
          <ThemeToggle />
        </div>
      </div>
    </header>
  );
});
