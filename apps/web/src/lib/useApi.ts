import { useAuth, useOrganization } from "@clerk/clerk-react";
import { useMemo } from "react";
import { createApi, type Api } from "./api.js";

/**
 * An API client bound to the signed-in user and their active organization.
 *
 * Rebuilt when the active org changes, so every request carries the right
 * scope without callers having to thread an org ID through by hand.
 */
export function useApi(): Api {
  const { getToken } = useAuth();
  const { organization } = useOrganization();

  return useMemo(
    () => createApi(() => getToken(), organization?.id ?? null),
    [getToken, organization?.id],
  );
}
