import { redirect } from "next/navigation";

/** Legacy /download → /desktop (weldsuite.org-style desktop page). */
export default function DownloadRedirect() {
  redirect("/desktop");
}
