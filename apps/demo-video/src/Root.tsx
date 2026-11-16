import { Composition } from "remotion";
import { DURATION, Demo } from "./Demo";

export function Root() {
  return <Composition id="Demo" component={Demo} durationInFrames={DURATION} fps={30} width={1920} height={1080} />;
}
