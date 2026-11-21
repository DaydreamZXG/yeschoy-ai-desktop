import { useEffect, useState } from "react";
import { prefersCalm, sound } from "./sound";

/** 台词一个字一个字打出来。关了动效的人直接看到整句。 */
export function useTyped(text: string, step = 28): string {
  const [shown, setShown] = useState(prefersCalm() ? text : "");
  useEffect(() => {
    if (prefersCalm()) {
      setShown(text);
      return;
    }
    let count = 0;
    setShown("");
    const timer = setInterval(() => {
      count += 1;
      setShown(text.slice(0, count));
      if (count % 3 === 0) sound.tick();
      if (count >= text.length) clearInterval(timer);
    }, step);
    return () => clearInterval(timer);
  }, [text, step]);
  return shown;
}
