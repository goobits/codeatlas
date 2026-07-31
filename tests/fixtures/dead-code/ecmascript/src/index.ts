import { aliased } from "@fixture/alias";
import defaultThing from "./defaultThing";
import * as namespace from "./namespace";
import "./sideEffect";
import { used } from "./used";

const common = require("./common.cjs");
const workerUrl = new URL("./worker.ts", import.meta.url);

aliased();
defaultThing();
namespace.run();
common.run();
used();
void workerUrl;
void import("./lazy");

export { publicApi } from "./publicApi";
