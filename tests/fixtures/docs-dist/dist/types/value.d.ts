/** Create a shipped declaration value. */
declare function createValue(options: ValueOptions): string;
/** Options accepted by the shipped declaration value. */
interface ValueOptions {
    label: string;
}
export { createValue as createShippedValue, ValueOptions as ShippedValueOptions };
