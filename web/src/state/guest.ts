import { createContextId } from "@builder.io/qwik";

export interface GuestState {
  /** `anonymous` means `/v1/me` returned 401: no session cookie, or it expired. */
  status: "loading" | "joined" | "anonymous" | "error";
  clientId: string;
  displayName: string;
}

export const GuestContext = createContextId<GuestState>("tse.guest");
