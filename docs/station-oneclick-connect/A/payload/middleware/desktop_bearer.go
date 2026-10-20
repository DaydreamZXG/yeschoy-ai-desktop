// Code generated for YesChoy desktop-connect patch. Marker: YESCHOY_DESKTOP_CONNECT
package middleware

import (
	"strings"
	"time"

	"__NEW_API_MODULE__/common"
	"__NEW_API_MODULE__/model"

	"github.com/gin-gonic/gin"
)

// DesktopBearerAuth lets "Authorization: Bearer <desktop access token>" identify
// the user on dashboard read/manage routes. Mount early on /api (apiRouter.Use).
// UserAuth must skip when desktop_session_id is already set (installer patches that).
func DesktopBearerAuth() gin.HandlerFunc {
	return func(c *gin.Context) {
		h := c.GetHeader("Authorization")
		if !strings.HasPrefix(h, "Bearer ") || c.GetHeader("New-Api-User") != "" {
			c.Next()
			return
		}
		raw := strings.TrimSpace(strings.TrimPrefix(h, "Bearer "))
		if raw == "" {
			c.Next()
			return
		}
		var s model.DesktopSession
		err := model.DB.Where(
			"access_hash = ? AND revoked_at = 0 AND access_expires_at > ?",
			model.HashDesktopToken(raw),
			time.Now().Unix(),
		).First(&s).Error
		if err != nil {
			c.Next()
			return
		}
		user, err := model.GetUserById(s.UserId, false)
		if err != nil || user == nil || user.Status != common.UserStatusEnabled {
			c.Next()
			return
		}
		c.Set("id", user.Id)
		c.Set("username", user.Username)
		c.Set("role", user.Role)
		c.Set("group", user.Group)
		c.Set("desktop_session_id", s.Id)
		c.Next()
	}
}
