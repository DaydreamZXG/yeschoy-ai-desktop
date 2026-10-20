// Code generated for YesChoy desktop-connect patch. Marker: YESCHOY_DESKTOP_CONNECT
// Draft aligned with yeschoy-ai-desktop/docs/patch/new-api-desktop-connect/
package controller

import (
	"crypto/rand"
	"encoding/base64"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"__NEW_API_MODULE__/common"
	"__NEW_API_MODULE__/model"

	"github.com/gin-gonic/gin"
)

const (
	connectCodeTTL    = 300 * time.Second
	desktopAccessTTL  = 15 * time.Minute
	desktopRefreshTTL = 30 * 24 * time.Hour
)

func randomDesktopToken(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return base64.RawURLEncoding.EncodeToString(b)
}

type desktopConnectCode struct {
	UserId    int    `json:"user_id"`
	Origin    string `json:"origin"`
	ExpiresAt int64  `json:"expires_at"`
}

var memDesktopCodes sync.Map

func putDesktopConnectCode(code string, v desktopConnectCode) error {
	key := "desktop:connect:" + model.HashDesktopToken(code)
	if common.RedisEnabled {
		raw, err := common.Marshal(v)
		if err != nil {
			return err
		}
		return common.RedisSet(key, string(raw), connectCodeTTL)
	}
	memDesktopCodes.Store(key, v)
	return nil
}

func takeDesktopConnectCode(code string) (desktopConnectCode, bool) {
	key := "desktop:connect:" + model.HashDesktopToken(code)
	var v desktopConnectCode
	if common.RedisEnabled {
		raw, err := common.RedisGet(key)
		if err != nil || common.UnmarshalJsonStr(raw, &v) != nil {
			return v, false
		}
		_ = common.RedisDel(key)
	} else {
		x, ok := memDesktopCodes.LoadAndDelete(key)
		if !ok {
			return v, false
		}
		v = x.(desktopConnectCode)
	}
	return v, v.ExpiresAt > time.Now().Unix()
}

func desktopRequestOrigin(c *gin.Context) string {
	host := strings.ToLower(c.Request.Host)
	// Prefer proto from reverse proxy when present
	proto := strings.ToLower(c.GetHeader("X-Forwarded-Proto"))
	if proto != "http" && proto != "https" {
		if c.Request.TLS != nil {
			proto = "https"
		} else {
			proto = "https" // desktop clients require HTTPS origins in practice
		}
	}
	return proto + "://" + host
}

// DesktopConnectBootstrap GET /api/desktop/v2/bootstrap
func DesktopConnectBootstrap(c *gin.Context) {
	origin := desktopRequestOrigin(c)
	c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{
		"schema_version":                 2,
		"service":                        "yeschoy-desktop",
		"contract_id":                    "desktop-integration-v2",
		"minimum_client_version":         "0.5.0",
		"device_authorization_available": false,
		"oauth_loopback_pkce_available":  false,
		"account_read":                   true,
		"usage_read":                     true,
		"models_read":                    true,
		"pricing_read":                   true,
		"tool_keys_manage":               true,
		"official_usd_cny_rate":          7.0,
		"authorization_start_path":       "/api/desktop/v2/device-authorizations",
		"authorization_token_path":       "/api/desktop/v2/device-authorizations/token",
		"session_refresh_path":           "/api/desktop/v2/sessions/refresh",
		"session_logout_path":            "/api/desktop/v2/sessions/current",
		"account_path":                   "/api/user/self",
		"usage_summary_path":             "/api/data/self",
		"usage_records_path":             "/api/log/self",
		"models_path":                    "/api/user/models",
		"pricing_path":                   "/api/pricing",
		"tool_keys_path":                 "/api/token/",
		"wallet_url":                     origin + "/console/topup",
	}})
}

// IssueDesktopConnectCode POST /api/desktop/v2/connect-codes (UserAuth)
func IssueDesktopConnectCode(c *gin.Context) {
	userId := c.GetInt("id")
	if userId <= 0 {
		c.JSON(http.StatusUnauthorized, gin.H{"success": false, "code": "session_required", "message": "请先登录"})
		return
	}
	code := randomDesktopToken(32)
	origin := desktopRequestOrigin(c)
	exp := time.Now().Add(connectCodeTTL).Unix()
	if err := putDesktopConnectCode(code, desktopConnectCode{UserId: userId, Origin: origin, ExpiresAt: exp}); err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"success": false, "code": "connect_code_store_failed"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{
		"code": code, "origin": origin, "expiresAt": exp, "ttlSeconds": int(connectCodeTTL.Seconds()),
	}})
}

func issueDesktopBundle(userId int, platform, version string) (gin.H, error) {
	now := time.Now()
	access, refresh := randomDesktopToken(32), randomDesktopToken(32)
	s := model.DesktopSession{
		Id:               common.GetUUID(),
		UserId:           userId,
		AccessHash:       model.HashDesktopToken(access),
		AccessExpiresAt:  now.Add(desktopAccessTTL).Unix(),
		RefreshHash:      model.HashDesktopToken(refresh),
		RefreshExpiresAt: now.Add(desktopRefreshTTL).Unix(),
		DevicePlatform:   platform,
		ClientVersion:    version,
		CreatedAt:        now.Unix(),
		LastSeenAt:       now.Unix(),
	}
	if err := model.DB.Create(&s).Error; err != nil {
		return nil, err
	}
	return gin.H{
		"accessToken":      access,
		"tokenType":        "Bearer",
		"accessExpiresAt":  s.AccessExpiresAt,
		"refreshToken":     refresh,
		"refreshExpiresAt": s.RefreshExpiresAt,
		"sessionId":        s.Id,
	}, nil
}

// ExchangeDesktopConnectCode POST /api/desktop/v2/connect-codes/exchange
func ExchangeDesktopConnectCode(c *gin.Context) {
	var req struct {
		Code          string `json:"code"`
		DeviceId      string `json:"deviceId"`
		Platform      string `json:"platform"`
		ClientVersion string `json:"clientVersion"`
	}
	if err := c.ShouldBindJSON(&req); err != nil || len(req.Code) < 32 {
		c.JSON(http.StatusBadRequest, gin.H{"success": false, "code": "connect_code_expired"})
		return
	}
	v, ok := takeDesktopConnectCode(req.Code)
	if !ok {
		c.JSON(http.StatusBadRequest, gin.H{"success": false, "code": "connect_code_expired"})
		return
	}
	if u, err := url.Parse(v.Origin); err != nil || !strings.EqualFold(u.Host, c.Request.Host) {
		c.JSON(http.StatusForbidden, gin.H{"success": false, "code": "connect_origin_mismatch"})
		return
	}
	bundle, err := issueDesktopBundle(v.UserId, req.Platform, req.ClientVersion)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"success": false, "code": "server_error"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"success": true, "data": bundle})
}

// RefreshDesktopConnectSession POST /api/desktop/v2/sessions/refresh
func RefreshDesktopConnectSession(c *gin.Context) {
	var req struct {
		RefreshToken string `json:"refresh_token"`
	}
	_ = c.ShouldBindJSON(&req)
	var s model.DesktopSession
	now := time.Now()
	if req.RefreshToken == "" ||
		model.DB.Where("refresh_hash = ? AND revoked_at = 0 AND refresh_expires_at > ?", model.HashDesktopToken(req.RefreshToken), now.Unix()).First(&s).Error != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"success": false, "code": "session_expired"})
		return
	}
	access, refresh := randomDesktopToken(32), randomDesktopToken(32)
	s.AccessHash, s.AccessExpiresAt = model.HashDesktopToken(access), now.Add(desktopAccessTTL).Unix()
	s.RefreshHash, s.RefreshExpiresAt = model.HashDesktopToken(refresh), now.Add(desktopRefreshTTL).Unix()
	s.LastSeenAt = now.Unix()
	_ = model.DB.Save(&s)
	c.JSON(http.StatusOK, gin.H{"success": true, "data": gin.H{
		"accessToken":      access,
		"tokenType":        "Bearer",
		"accessExpiresAt":  s.AccessExpiresAt,
		"refreshToken":     refresh,
		"refreshExpiresAt": s.RefreshExpiresAt,
		"sessionId":        s.Id,
	}})
}

// RevokeDesktopConnectSession DELETE /api/desktop/v2/sessions/current
func RevokeDesktopConnectSession(c *gin.Context) {
	id := c.GetString("desktop_session_id")
	if id != "" {
		_ = model.DB.Model(&model.DesktopSession{}).Where("id = ?", id).Update("revoked_at", time.Now().Unix())
	}
	c.JSON(http.StatusOK, gin.H{"success": true})
}
