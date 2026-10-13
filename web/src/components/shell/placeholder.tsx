import { component$ } from "@builder.io/qwik";
import { Link } from "@builder.io/qwik-city";

interface PlaceholderProps {
  title: string;
  lede: string;
  items: string[];
}

/** Stand-in for a page that ships in a later phase. */
export const Placeholder = component$<PlaceholderProps>(
  ({ title, lede, items }) => {
    return (
      <div class="wrap">
        <section class="placeholder" aria-labelledby="placeholder-title">
          <h1 id="placeholder-title">{title}</h1>
          <p class="lede">{lede}</p>
          <ul>
            {items.map((item) => (
              <li key={item}>{item}</li>
            ))}
          </ul>
          <p class="phase muted">
            This page is coming in a later phase. Until then,{" "}
            <Link href="/trade/">go to Trade</Link> to place orders and watch
            the book.
          </p>
        </section>
      </div>
    );
  },
);
