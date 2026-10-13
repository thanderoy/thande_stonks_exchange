import {
  component$,
  Slot,
  useContextProvider,
  useStore,
  useVisibleTask$,
} from "@builder.io/qwik";
import { getMe } from "~/api/client";
import { Disclaimer } from "~/components/shell/disclaimer";
import { TopBar } from "~/components/shell/top-bar";
import { GuestContext, type GuestState } from "~/state/guest";

export default component$(() => {
  const guest = useStore<GuestState>({
    status: "loading",
    clientId: "",
    displayName: "",
  });
  useContextProvider(GuestContext, guest);

  // The site is prerendered, so who is visiting is only known in the browser.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(
    async () => {
      try {
        const me = await getMe();
        if (me) {
          guest.clientId = me.client_id;
          guest.displayName = me.display_name;
          guest.status = "joined";
        } else {
          guest.status = "anonymous";
        }
      } catch {
        guest.status = "error";
      }
    },
    { strategy: "document-ready" },
  );

  return (
    <>
      <a class="skip-link" href="#main">
        Skip to content
      </a>
      <TopBar />
      <main id="main" tabIndex={-1}>
        <Slot />
      </main>
      <Disclaimer />
    </>
  );
});
