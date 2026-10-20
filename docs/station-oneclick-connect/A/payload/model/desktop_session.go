// Code generated for YesChoy desktop-connect patch. DO NOT EDIT BY HAND unless you know why.
// Marker: YESCHOY_DESKTOP_CONNECT
package model

import (
	"crypto/sha256"
	"encoding/hex"
)

// DesktopSession stores hashed desktop access/refresh tokens for one-click connect.
// Table: desktop_sessions
type DesktopSession struct {
	Id               string `json:"id" gorm:"primaryKey;size:36"`
	UserId           int    `json:"user_id" gorm:"index"`
	AccessHash       string `json:"-" gorm:"size:64;uniqueIndex"`
	AccessExpiresAt  int64  `json:"-"`
	RefreshHash      string `json:"-" gorm:"size:64;uniqueIndex"`
	RefreshExpiresAt int64  `json:"-"`
	RevokedAt        int64  `json:"-" gorm:"index"`
	DevicePlatform   string `json:"device_platform" gorm:"size:16"`
	ClientVersion    string `json:"client_version" gorm:"size:32"`
	CreatedAt        int64  `json:"created_at"`
	LastSeenAt       int64  `json:"last_seen_at"`
}

func (DesktopSession) TableName() string {
	return "desktop_sessions"
}

// HashDesktopToken SHA-256 hex of a raw access/refresh/connect token.
func HashDesktopToken(t string) string {
	s := sha256.Sum256([]byte(t))
	return hex.EncodeToString(s[:])
}
