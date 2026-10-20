// Code generated for YesChoy desktop-connect patch. Marker: YESCHOY_DESKTOP_CONNECT
package controller

import (
	_ "embed"
	"net/http"

	"github.com/gin-gonic/gin"
)

//go:embed desktop_connect.html
var desktopConnectPageHTML []byte

// ServeDesktopConnectPage serves GET /desktop/connect (fixed path for YesChoy desktop).
func ServeDesktopConnectPage(c *gin.Context) {
	c.Header("Cache-Control", "no-store")
	c.Data(http.StatusOK, "text/html; charset=utf-8", desktopConnectPageHTML)
}
