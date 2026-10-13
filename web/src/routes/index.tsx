import { $, component$, useContext, useSignal } from "@builder.io/qwik";
import { Link, useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { ApiError, join } from "~/api/client";
import { NAME_MAX, validateDisplayName } from "~/lib/format";
import { GuestContext } from "~/state/guest";

export default component$(() => {
  const guest = useContext(GuestContext);
  const nav = useNavigate();
  const name = useSignal("");
  const error = useSignal("");
  const busy = useSignal(false);

  const submit = $(async () => {
    const problem = validateDisplayName(name.value);
    if (problem) {
      error.value = problem;
      return;
    }
    busy.value = true;
    error.value = "";
    try {
      const me = await join(name.value.trim());
      guest.clientId = me.client_id;
      guest.displayName = me.display_name;
      guest.status = "joined";
      await nav("/trade/");
    } catch (err) {
      error.value =
        err instanceof ApiError
          ? err.message
          : "Something went wrong. Reload the page and try again.";
    } finally {
      busy.value = false;
    }
  });

  return (
    <div class="wrap">
      <section class="landing" aria-labelledby="intro-title">
        <h1 id="intro-title">A stock exchange you can trade on</h1>
        <p class="lede">
          Thande Stonks Exchange is a matching engine written in Rust. Place
          orders against bots and other visitors, and watch the order book and
          trade tape update live.
        </p>
        <ul class="facts">
          <li>
            Three symbols: <span class="num">AAPL</span>,{" "}
            <span class="num">MSFT</span> and <span class="num">GOOGL</span>.
          </li>
          <li>Orders match by price, then by time of arrival.</li>
          <li>
            No account or email. Pick a display name to get a guest session.
          </li>
        </ul>

        {guest.status === "joined" && (
          <div class="continue">
            <span>
              You joined as <strong>{guest.displayName}</strong>.
            </span>
            <Link href="/trade/" class="btn btn-primary">
              Continue as {guest.displayName}
            </Link>
          </div>
        )}

        <form
          class="join"
          preventdefault:submit
          onSubmit$={submit}
          noValidate
          aria-describedby="join-help"
        >
          <h2>
            {guest.status === "joined" ? "Or join with a new name" : "Join"}
          </h2>
          <div class="join-row">
            <div class="field">
              <label for="display-name">Display name</label>
              <input
                id="display-name"
                class="input"
                name="display_name"
                autoComplete="nickname"
                maxLength={NAME_MAX}
                required
                bind:value={name}
                aria-invalid={error.value ? "true" : undefined}
                aria-describedby="join-help join-error"
              />
            </div>
            <button type="submit" class="btn btn-primary" disabled={busy.value}>
              {busy.value ? "Joining" : "Join and trade"}
            </button>
          </div>
          <p id="join-help" class="muted notice">
            2 to 24 characters: letters, digits, spaces, underscores or hyphens.
            Other visitors see this name next to your trades.
          </p>
          <p id="join-error" class="notice error" aria-live="polite">
            {error.value}
          </p>
          {guest.status === "error" && (
            <p class="notice error" aria-live="polite">
              Can't reach the exchange server right now. You can still try to
              join; if it fails, try again in a minute.
            </p>
          )}
        </form>
      </section>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Thande Stonks Exchange",
  meta: [
    {
      name: "description",
      content:
        "A simulated stock exchange. Join with a display name and trade against bots and other visitors.",
    },
  ],
};
