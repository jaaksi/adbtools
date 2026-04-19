// Google OAuth 配置
// 注意：你需要在 Google Cloud Console 创建 OAuth 2.0 客户端 ID
// https://console.cloud.google.com/apis/credentials

export const GOOGLE_AUTH_CONFIG = {
  // 替换为你的 Google Client ID
  clientId: import.meta.env.VITE_GOOGLE_CLIENT_ID || '',
  
  // 作用域 - 根据需要调整
  scope: 'email profile openid',
  
  // 是否自动选择
  autoSelect: true,
  
  // 是否使用 FedCM (Federated Credential Management)
  useFedCM: true,
  
  // 上下文 - signin, signup, use
  context: 'signin',
};

// 用户信息类型
export interface GoogleUserInfo {
  id: string;
  email: string;
  name: string;
  picture: string;
  givenName?: string;
  familyName?: string;
}

// 认证响应类型
export interface GoogleAuthResponse {
  credential: string;
  clientId: string;
  select_by: string;
}

// 检查是否配置了 Client ID
export function isGoogleAuthConfigured(): boolean {
  return !!GOOGLE_AUTH_CONFIG.clientId;
}
