/** Official YesChoy user QQ group (community UI). */
export const QQ_GROUP_ID = "1065665694";

/** Set `VITE_YESCHOY_QQ_GROUP=0` to omit QQ community UI when assets are not shipped. */
export function qqCommunityConfigAvailable(): boolean {
  return (
    import.meta.env.VITE_YESCHOY_QQ_GROUP !== "0" && QQ_GROUP_ID.length > 0
  );
}
