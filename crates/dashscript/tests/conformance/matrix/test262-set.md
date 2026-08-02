# DashScript Conformance Matrix

- 350 features: **350** supported, **0** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## set

| feature                                                                                                   | status       | detail / note                              |
| --------------------------------------------------------------------------------------------------------- | ------------ | ------------------------------------------ |
| test262.test.built-ins.set.bigint-number-same-value                                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.constructor                                                                    | 🟢 supported |                                            |
| test262.test.built-ins.set.is-a-constructor                                                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.properties-of-the-set-prototype-object                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.proto-from-ctor-realm                                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype-of-set                                                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.add                                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.does-not-have-setdata-internal-slot-array                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.does-not-have-setdata-internal-slot-map                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.does-not-have-setdata-internal-slot-object                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.does-not-have-setdata-internal-slot-set-prototype                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.does-not-have-setdata-internal-slot-weakset                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.not-a-constructor                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.preserves-insertion-order                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.returns-this                                                     | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.add.returns-this-when-ignoring-duplicate                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-boolean                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-null                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-number                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-string                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-symbol                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.this-not-object-throw-undefined                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.add.will-not-add-duplicate-entry                                     | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.add.will-not-add-duplicate-entry-initial-iterable                    | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.add.will-not-add-duplicate-entry-normalizes-zero                     | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.clear.clear                                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.clears-all-contents                                            | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.clear.clears-all-contents-from-iterable                              | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.clear.clears-an-empty-set                                            | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.clear.does-not-have-setdata-internal-slot-array                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.does-not-have-setdata-internal-slot-map                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.does-not-have-setdata-internal-slot-object                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.does-not-have-setdata-internal-slot-set.prototype              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.does-not-have-setdata-internal-slot-weakset                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.not-a-constructor                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.returns-undefined                                              | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-boolean                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-null                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-number                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-string                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-symbol                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.clear.this-not-object-throw-undefined                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.constructor.set-prototype-constructor-intrinsic                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.delete                                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.delete-entry                                                  | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.delete.delete-entry-initial-iterable                                 | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.delete.delete-entry-normalizes-zero                                  | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.delete.does-not-have-setdata-internal-slot-array                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.does-not-have-setdata-internal-slot-map                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.does-not-have-setdata-internal-slot-object                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.does-not-have-setdata-internal-slot-set-prototype             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.does-not-have-setdata-internal-slot-weakset                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.not-a-constructor                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.returns-false-when-delete-is-noop                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.delete.returns-true-when-delete-operation-occurs                     | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-boolean                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-null                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-number                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-string                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-symbol                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.delete.this-not-object-throw-undefined                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.add-not-called                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.allows-set-like-class                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.allows-set-like-object                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.array-throws                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.builtins                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.called-with-object                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.combines-empty-sets                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.combines-itself                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.combines-map                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.combines-same-sets                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.combines-sets                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.converts-negative-zero                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.difference                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.has-is-callable                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.keys-is-callable                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.length                                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.name                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.not-a-constructor                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.receiver-not-set                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.require-internal-slot                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.set-like-array                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.size-is-a-number                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.subclass                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.subclass-receiver-methods                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.difference.subclass-symbol-species                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.does-not-have-setdata-internal-slot-array                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.does-not-have-setdata-internal-slot-map                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.does-not-have-setdata-internal-slot-object                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.does-not-have-setdata-internal-slot-set-prototype            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.does-not-have-setdata-internal-slot-weakset                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.entries                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.not-a-constructor                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.returns-iterator                                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.entries.returns-iterator-empty                                       | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-boolean                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-null                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-number                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-string                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-symbol                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.entries.this-not-object-throw-undefined                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-boolean                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-null                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-number                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-string                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-symbol                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.callback-not-callable-undefined                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.does-not-have-setdata-internal-slot-array                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.does-not-have-setdata-internal-slot-map                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.does-not-have-setdata-internal-slot-object                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.does-not-have-setdata-internal-slot-set-prototype            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.does-not-have-setdata-internal-slot-weakset                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.foreach                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-in-insertion-order                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-in-iterable-entry-order                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-values-added-after-foreach-begins                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-values-deleted-then-readded                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-values-not-deleted                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.iterates-values-revisits-after-delete-re-add                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.not-a-constructor                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.returns-undefined                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-arg-explicit                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-arg-explicit-cannot-override-lexical-this-arrow         | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.foreach.this-non-strict                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-boolean                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-null                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-number                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-string                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-symbol                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-not-object-throw-undefined                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.this-strict                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.foreach.throws-when-callback-throws                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.does-not-have-setdata-internal-slot-array                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.does-not-have-setdata-internal-slot-map                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.does-not-have-setdata-internal-slot-object                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.does-not-have-setdata-internal-slot-set-prototype                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.does-not-have-setdata-internal-slot-weakset                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.has                                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.not-a-constructor                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.returns-false-when-undefined-added-deleted-not-present-undefined | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-boolean                     | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-nan                         | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-null                        | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-number                      | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-string                      | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-symbol                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.returns-false-when-value-not-present-undefined                   | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-boolean                          | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-nan                              | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-null                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-number                           | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-string                           | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-symbol                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.returns-true-when-value-present-undefined                        | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-boolean                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-null                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-number                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-string                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-symbol                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.has.this-not-object-throw-undefined                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.add-not-called                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.allows-set-like-class                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.allows-set-like-object                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.array-throws                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.builtins                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.called-with-object                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.combines-empty-sets                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.combines-itself                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.combines-map                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.combines-same-sets                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.combines-sets                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.converts-negative-zero                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.has-is-callable                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.intersection                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.keys-is-callable                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.length                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.name                                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.not-a-constructor                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.receiver-not-set                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.require-internal-slot                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.set-like-array                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.size-is-a-number                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.subclass                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.subclass-receiver-methods                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.intersection.subclass-symbol-species                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.allows-set-like-class                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.allows-set-like-object                                | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.array-throws                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.builtins                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.called-with-object                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.compares-empty-sets                                   | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.compares-itself                                       | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.compares-map                                          | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.compares-same-sets                                    | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.compares-sets                                         | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.converts-negative-zero                                | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.has-is-callable                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.isdisjointfrom                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.keys-is-callable                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.length                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.name                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.not-a-constructor                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.receiver-not-set                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.require-internal-slot                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.set-like-array                                        | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.set-like-class-mutation                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.set-like-class-order                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.set-like-iter-return                                  | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.isdisjointfrom.size-is-a-number                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.isdisjointfrom.subclass-receiver-methods                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.allows-set-like-class                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.allows-set-like-object                                    | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.array-throws                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.builtins                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.called-with-object                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.compares-empty-sets                                       | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.compares-itself                                           | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.compares-map                                              | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.compares-same-sets                                        | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.compares-sets                                             | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.has-is-callable                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.issubsetof                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.keys-is-callable                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.length                                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.name                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.not-a-constructor                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.receiver-not-set                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.require-internal-slot                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.set-like-array                                            | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issubsetof.set-like-class-mutation                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.set-like-class-order                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.size-is-a-number                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issubsetof.subclass-receiver-methods                                 | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.allows-set-like-class                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.allows-set-like-object                                  | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.array-throws                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.builtins                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.called-with-object                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.compares-empty-sets                                     | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.compares-itself                                         | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.compares-map                                            | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.compares-same-sets                                      | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.compares-sets                                           | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.converts-negative-zero                                  | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.has-is-callable                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.issupersetof                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.keys-is-callable                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.length                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.name                                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.not-a-constructor                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.receiver-not-set                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.require-internal-slot                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.set-like-array                                          | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.set-like-class-mutation                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.set-like-class-order                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.set-like-iter-return                                    | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.issupersetof.size-is-a-number                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.issupersetof.subclass-receiver-methods                               | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.keys.keys                                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.size.returns-count-of-present-values-before-after-add-delete         | 🟢 supported |                                            |
| test262.test.built-ins.set.prototype.size.returns-count-of-present-values-by-insertion                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.size.returns-count-of-present-values-by-iterable                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.size.size                                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symbol.iterator                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symbol.iterator.not-a-constructor                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symbol.tostringtag                                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symbol.tostringtag.property-descriptor                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.add-not-called                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.allows-set-like-class                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.allows-set-like-object                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.array-throws                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.builtins                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.called-with-object                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.combines-empty-sets                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.combines-itself                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.combines-map                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.combines-same-sets                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.combines-sets                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.converts-negative-zero                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.has-is-callable                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.keys-is-callable                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.length                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.name                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.not-a-constructor                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.receiver-not-set                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.require-internal-slot                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.set-like-array                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.size-is-a-number                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.subclass                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.subclass-receiver-methods                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.subclass-symbol-species                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.symmetricdifference.symmetricdifference                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.add-not-called                                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.allows-set-like-class                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.allows-set-like-object                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.appends-new-values                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.array-throws                                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.builtins                                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.called-with-object                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.combines-empty-sets                                            | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.combines-itself                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.combines-map                                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.combines-same-sets                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.combines-sets                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.converts-negative-zero                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.has-is-callable                                                | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.keys-is-callable                                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.length                                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.name                                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.not-a-constructor                                              | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.receiver-not-set                                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.require-internal-slot                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.set-like-array                                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.size-is-a-number                                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.subclass                                                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.subclass-receiver-methods                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.subclass-symbol-species                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.union.union                                                          | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.does-not-have-setdata-internal-slot-array                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.does-not-have-setdata-internal-slot-map                       | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.does-not-have-setdata-internal-slot-object                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.does-not-have-setdata-internal-slot-set-prototype             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.does-not-have-setdata-internal-slot-weakset                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.not-a-constructor                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.returns-iterator                                              | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.values.returns-iterator-empty                                        | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-boolean                                 | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-null                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-number                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-string                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-symbol                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.this-not-object-throw-undefined                               | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.values                                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.prototype.values.values-iteration-mutable                                      | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.set-does-not-throw-when-add-is-not-callable                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-get-add-method-failure                                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterable                                                                   | 🟢 supported |                                            |
| test262.test.built-ins.set.set-iterable-calls-add                                                         | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterable-empty-does-not-call-add                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterable-throws-when-add-is-not-callable                                   | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterator-close-after-add-failure                                           | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterator-next-failure                                                      | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-iterator-value-failure                                                     | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-newtarget                                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.set-no-iterable                                                                | 🟢 supported | engine fallback after static build failure |
| test262.test.built-ins.set.set-undefined-newtarget                                                        | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.symbol.species.return-value                                                    | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.symbol.species.symbol-species                                                  | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.symbol.species.symbol-species-name                                             | 🟢 supported | via rquickjs engine                        |
| test262.test.built-ins.set.valid-values                                                                   | 🟢 supported | via rquickjs engine                        |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
