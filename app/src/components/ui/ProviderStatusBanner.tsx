import Badge from "./Badge";
import { messages } from "../../messages";

export type ProviderStatus = "free" | "requires-choice" | "needs-reconnect";

interface ProviderStatusBannerProps {
  status: ProviderStatus;
  onConnect: () => void;
}

export default function ProviderStatusBanner({ status, onConnect }: ProviderStatusBannerProps) {
  if (status === "free") {
    return (
      <div className="provider-status-banner">
        <Badge tone="ok">{messages.provider.banner.freeModel}</Badge>
      </div>
    );
  }

  return (
    <div className="provider-status-banner">
      <p>
        {status === "requires-choice"
          ? messages.provider.banner.noAiConnected
          : messages.provider.banner.reconnectRequired}
      </p>
      <button type="button" className="primary" onClick={onConnect}>
        {messages.provider.banner.connectAction}
      </button>
    </div>
  );
}
