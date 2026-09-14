import { Navigate, Route, Routes } from "react-router-dom";
import { DashboardLayout } from "./marketing/DashboardLayout.js";
import { DownloadPage } from "./marketing/DownloadPage.js";
import { LandingPage } from "./marketing/LandingPage.js";
import { PricingPage } from "./marketing/PricingPage.js";
import { PrivacyPage } from "./marketing/PrivacyPage.js";
import { SignInPage } from "./marketing/SignInPage.js";
import { Dictionary } from "./routes/Dictionary.js";
import { History } from "./routes/History.js";
import { LinkDevice } from "./routes/LinkDevice.js";
import { Team } from "./routes/Team.js";
import { Usage } from "./routes/Usage.js";

export function App() {
  return (
    <Routes>
      <Route path="/" element={<LandingPage />} />
      <Route path="/download" element={<DownloadPage />} />
      <Route path="/pricing" element={<PricingPage />} />
      <Route path="/sign-in/*" element={<SignInPage />} />
      <Route path="/privacy" element={<PrivacyPage />} />
      <Route element={<DashboardLayout />}>
        <Route path="/link" element={<LinkDevice />} />
        <Route path="/dictionary" element={<Dictionary />} />
        <Route path="/history" element={<History />} />
        <Route path="/team" element={<Team />} />
        <Route path="/usage" element={<Usage />} />
      </Route>
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  );
}
