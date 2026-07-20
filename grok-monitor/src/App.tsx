import { useState, useEffect } from "react";
import "./App.css";

// Types matching Rust models
interface Account {
  id: string;
  display_name: string;
  email?: string;
  usage?: UsageSnapshot;
  last_polled?: string;
  poll_interval_secs: number;
  is_active: boolean;
  account_type: "Consumer" | "API" | "Enterprise";
}

interface UsageSnapshot {
  remaining_queries?: number;
  total_queries?: number;
  remaining_tokens?: number;
  total_tokens?: number;
  reset_at?: string;
  model_breakdown: Record<string, ModelUsage>;
  last_updated: string;
}

interface ModelUsage {
  model_name: string;
  queries_used: number;
  tokens_used?: number;
}

interface AppSettings {
  opacity: number;
  always_on_top: boolean;
  default_poll_interval: number;
  theme: "Dark" | "Light" | "System";
  compact_mode: boolean;
  auto_start: boolean;
}

function App() {
  const [accounts] = useState<Account[]>([]);
  const [settings, setSettings] = useState<AppSettings>({
    opacity: 0.9,
    always_on_top: true,
    default_poll_interval: 30,
    theme: "Dark",
    compact_mode: true,
    auto_start: false,
  });
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  // Fetch accounts (placeholder - will be implemented with Tauri commands)
  const fetchAccounts = async () => {
    try {
      // TODO: Implement Tauri command to get accounts
      setLastUpdated(new Date());
    } catch (error) {
      console.error("Failed to fetch accounts:", error);
    }
  };

  // Auto-refresh accounts
  useEffect(() => {
    const interval = setInterval(fetchAccounts, settings.default_poll_interval * 1000);
    fetchAccounts(); // Initial fetch
    return () => clearInterval(interval);
  }, [settings.default_poll_interval]);

  // Calculate usage percentage
  const getUsagePercentage = (usage?: UsageSnapshot): number | null => {
    if (!usage?.remaining_queries || !usage?.total_queries) return null;
    return ((usage.total_queries - usage.remaining_queries) / usage.total_queries) * 100;
  };

  // Get color based on usage
  const getUsageColor = (percentage: number | null): string => {
    if (percentage === null) return "#888";
    if (percentage < 50) return "#4ade80"; // Green
    if (percentage < 80) return "#fbbf24"; // Yellow
    return "#ef4444"; // Red
  };

  // Format time until reset
  const formatTimeUntilReset = (resetAt?: string): string => {
    if (!resetAt) return "不明";
    const reset = new Date(resetAt);
    const now = new Date();
    const diff = reset.getTime() - now.getTime();
    
    if (diff <= 0) return "リセット済み";
    
    const hours = Math.floor(diff / (1000 * 60 * 60));
    const minutes = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
    const seconds = Math.floor((diff % (1000 * 60)) / 1000);
    
    return `${hours.toString().padStart(2, '0')}:${minutes.toString().padStart(2, '0')}:${seconds.toString().padStart(2, '0')}`;
  };

  return (
    <div 
      className="app-container" 
      style={{ opacity: settings.opacity }}
    >
      <header className="app-header">
        <h1>Grok Monitor</h1>
        <div className="window-controls">
          <span className="last-updated">
            更新：{lastUpdated ? `${Math.floor((Date.now() - lastUpdated.getTime()) / 1000)}秒前` : "未更新"}
          </span>
        </div>
      </header>

      <div className="accounts-list">
        {accounts.length === 0 ? (
          <div className="no-accounts">
            <p>アカウントが登録されていません</p>
            <button onClick={() => {/* TODO: Open OAuth flow */}}>
              ＋ アカウント追加
            </button>
          </div>
        ) : (
          accounts.map((account) => {
            const percentage = getUsagePercentage(account.usage);
            const color = getUsageColor(percentage);
            
            return (
              <div key={account.id} className="account-card">
                <div className="account-header">
                  <span className={`status-dot ${account.is_active ? 'active' : 'inactive'}`} />
                  <span className="account-name">{account.display_name}</span>
                  <span className="account-type">{account.account_type}</span>
                </div>
                
                {account.usage && (
                  <div className="usage-info">
                    <div className="usage-stats">
                      <span>残：{account.usage.remaining_queries ?? "?"}/{account.usage.total_queries ?? "?"} queries</span>
                      <div 
                        className="usage-bar" 
                        style={{ 
                          width: '100px',
                          backgroundColor: '#333',
                          display: 'inline-block',
                          marginLeft: '10px',
                          height: '8px',
                          borderRadius: '4px',
                          overflow: 'hidden'
                        }}
                      >
                        <div 
                          style={{ 
                            width: `${percentage ?? 0}%`,
                            backgroundColor: color,
                            height: '100%',
                            transition: 'width 0.3s ease'
                          }}
                        />
                      </div>
                      <span style={{ marginLeft: '10px', color }}>
                        {percentage !== null ? `${Math.round(percentage)}%` : '-'}
                      </span>
                    </div>
                    <div className="reset-info">
                      リセット：{formatTimeUntilReset(account.usage.reset_at)} 後
                    </div>
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      <div className="controls">
        <div className="opacity-control">
          <label>透明度:</label>
          <input
            type="range"
            min="0.1"
            max="1"
            step="0.05"
            value={settings.opacity}
            onChange={(e) => setSettings({ ...settings, opacity: parseFloat(e.target.value) })}
          />
        </div>
        
        <button 
          className="refresh-btn"
          onClick={fetchAccounts}
        >
          更新
        </button>
      </div>
    </div>
  );
}

export default App;
