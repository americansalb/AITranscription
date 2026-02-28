/**
 * BillingPage — usage dashboard and subscription management.
 * Shows token consumption per role/provider, cost tracking, and plan upgrade.
 */

import { useEffect, useState } from "react";
import { useAuthStore } from "../lib/stores";
import * as api from "../lib/api";
import { ErrorBanner } from "../components/ErrorBanner";
import { LoadingSpinner } from "../components/LoadingSpinner";

interface UsageTier {
  name: string;
  limit: string;
  price: string;
  features: string[];
  current: boolean;
}

const TIERS: UsageTier[] = [
  {
    name: "Free",
    limit: "50K tokens/month",
    price: "Free",
    features: ["1 project", "4 roles", "Claude Haiku only"],
    current: false,
  },
  {
    name: "Pro",
    limit: "2M tokens/month",
    price: "$29/month",
    features: ["Unlimited projects", "All roles", "All providers", "Priority support"],
    current: false,
  },
  {
    name: "BYOK",
    limit: "Unlimited",
    price: "$9/month + your API keys",
    features: ["Your own API keys", "No token limits", "All features", "Data privacy"],
    current: false,
  },
];

function UsageMeter({ used, total, label }: { used: number; total: number; label: string }) {
  const percent = total > 0 ? Math.min((used / total) * 100, 100) : 0;
  const isWarning = percent > 80;
  const isDanger = percent > 95;

  return (
    <div style={{ marginBottom: "var(--space-4)" }}>
      <div style={{
        display: "flex",
        justifyContent: "space-between",
        marginBottom: "var(--space-1)",
        fontSize: "var(--text-sm)",
      }}>
        <span style={{ color: "var(--text-secondary)" }}>{label}</span>
        <span style={{
          color: isDanger ? "var(--error)" : isWarning ? "var(--warning)" : "var(--text-muted)",
          fontWeight: "var(--weight-medium)",
        }}>
          {formatTokens(used)} / {formatTokens(total)}
        </span>
      </div>
      <div style={{
        height: 6,
        background: "var(--bg-tertiary)",
        borderRadius: "var(--radius-full)",
        overflow: "hidden",
      }}
        role="progressbar"
        aria-valuenow={used}
        aria-valuemin={0}
        aria-valuemax={total}
        aria-label={`${label}: ${formatTokens(used)} of ${formatTokens(total)} used`}
      >
        <div style={{
          height: "100%",
          width: `${percent}%`,
          background: isDanger ? "var(--error)" : isWarning ? "var(--warning)" : "var(--accent)",
          borderRadius: "var(--radius-full)",
          transition: "width var(--transition-normal)",
        }} />
      </div>
    </div>
  );
}

function formatTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(0)}K`;
  return String(n);
}

export function BillingPage() {
  const user = useAuthStore((s) => s.user);
  const [subscription, setSubscription] = useState<api.SubscriptionStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [upgrading, setUpgrading] = useState(false);

  useEffect(() => {
    loadBilling();
  }, []);

  const loadBilling = async () => {
    setLoading(true);
    setError(null);
    try {
      const status = await api.getSubscriptionStatus();
      setSubscription(status);
    } catch (e) {
      setError(e instanceof api.ApiError ? e.userMessage : "Failed to load billing info");
    } finally {
      setLoading(false);
    }
  };

  const handleUpgrade = async (plan: string) => {
    setUpgrading(true);
    try {
      const { url } = await api.createCheckout(plan);
      window.location.href = url;
    } catch (e) {
      setError(e instanceof api.ApiError ? e.userMessage : "Failed to start checkout");
    } finally {
      setUpgrading(false);
    }
  };

  if (loading) return <LoadingSpinner label="Loading billing..." timeoutSeconds={10} />;

  const currentTier = subscription?.plan || "free";
  const tokensUsed = subscription?.usage?.tokens_used || 0;
  const tokensLimit = subscription?.usage?.tokens_limit || 50000;
  const costUsd = subscription?.usage?.cost_usd || 0;

  return (
    <>
      <div className="page-header">
        <h1 style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)" }}>Billing & Usage</h1>
      </div>

      <div className="page-body">
        {error && <ErrorBanner message={error} onRetry={loadBilling} onDismiss={() => setError(null)} />}

        {/* Usage overview */}
        <div className="card" style={{ marginBottom: "var(--space-6)" }}>
          <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-4)" }}>
            Current Usage
          </h2>

          <UsageMeter used={tokensUsed} total={tokensLimit} label="Tokens this month" />

          <div style={{
            display: "grid",
            gridTemplateColumns: "repeat(3, 1fr)",
            gap: "var(--space-4)",
            marginTop: "var(--space-4)",
          }}>
            <div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>Plan</div>
              <div style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", textTransform: "capitalize" }}>
                {currentTier}
              </div>
            </div>
            <div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>Cost this month</div>
              <div style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)" }}>
                ${costUsd.toFixed(2)}
              </div>
            </div>
            <div>
              <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)" }}>Account</div>
              <div style={{ fontSize: "var(--text-sm)", color: "var(--text-secondary)" }}>
                {user?.email}
              </div>
            </div>
          </div>
        </div>

        {/* Plan comparison */}
        <h2 style={{ fontSize: "var(--text-md)", fontWeight: "var(--weight-semibold)", marginBottom: "var(--space-4)" }}>
          Plans
        </h2>
        <div style={{
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(220px, 1fr))",
          gap: "var(--space-4)",
        }}>
          {TIERS.map((tier) => {
            const isCurrent = tier.name.toLowerCase() === currentTier;
            return (
              <div
                key={tier.name}
                className="card"
                style={{
                  borderColor: isCurrent ? "var(--accent)" : "var(--border)",
                  position: "relative",
                }}
              >
                {isCurrent && (
                  <span className="badge badge-accent" style={{ position: "absolute", top: "var(--space-3)", right: "var(--space-3)" }}>
                    Current
                  </span>
                )}
                <div style={{ fontSize: "var(--text-lg)", fontWeight: "var(--weight-bold)", marginBottom: "var(--space-1)" }}>
                  {tier.name}
                </div>
                <div style={{ fontSize: "var(--text-xl)", fontWeight: "var(--weight-bold)", color: "var(--accent)", marginBottom: "var(--space-1)" }}>
                  {tier.price}
                </div>
                <div style={{ fontSize: "var(--text-xs)", color: "var(--text-muted)", marginBottom: "var(--space-3)" }}>
                  {tier.limit}
                </div>
                <ul style={{ listStyle: "none", padding: 0, marginBottom: "var(--space-4)" }}>
                  {tier.features.map((f) => (
                    <li key={f} style={{
                      fontSize: "var(--text-sm)",
                      color: "var(--text-secondary)",
                      padding: "var(--space-1) 0",
                      display: "flex",
                      alignItems: "center",
                      gap: "var(--space-2)",
                    }}>
                      <span style={{ color: "var(--success)" }}>{"\u2713"}</span> {f}
                    </li>
                  ))}
                </ul>
                {!isCurrent && (
                  <button
                    className="btn btn-primary"
                    style={{ width: "100%" }}
                    onClick={() => handleUpgrade(tier.name.toLowerCase())}
                    disabled={upgrading}
                  >
                    {upgrading ? "Redirecting..." : `Upgrade to ${tier.name}`}
                  </button>
                )}
              </div>
            );
          })}
        </div>
      </div>
    </>
  );
}
