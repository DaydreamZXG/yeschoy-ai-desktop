// Code generated for YesChoy desktop-connect patch. Marker: YESCHOY_DESKTOP_CONNECT
package router

import (
	"__NEW_API_MODULE__/controller"
	"__NEW_API_MODULE__/middleware"

	"github.com/gin-gonic/gin"
)

// registerDesktopConnectRoutes mounts /api/desktop/v2/* for YesChoy one-click connect.
// Called from api-router.go via installer marker block.
func registerDesktopConnectRoutes(apiRouter *gin.RouterGroup) {
	d := apiRouter.Group("/desktop/v2")
	{
		d.GET("/bootstrap", controller.DesktopConnectBootstrap)
		d.POST("/connect-codes", middleware.UserAuth(), middleware.CriticalRateLimit(), controller.IssueDesktopConnectCode)
		d.POST("/connect-codes/exchange", middleware.CriticalRateLimit(), controller.ExchangeDesktopConnectCode)
		d.POST("/sessions/refresh", middleware.CriticalRateLimit(), controller.RefreshDesktopConnectSession)
		d.DELETE("/sessions/current", controller.RevokeDesktopConnectSession)
	}
}
