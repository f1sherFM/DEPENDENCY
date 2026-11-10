import { util } from "@/util";
import { shared } from "@shared";
import type { Model } from "@models/model";
import { legacy } from "./legacy.js";
import React from "react";
export const result: Model = { util, shared, legacy, React };
