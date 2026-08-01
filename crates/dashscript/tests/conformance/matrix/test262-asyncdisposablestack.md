# DashScript Conformance Matrix

- 76 features: **76** supported, **0** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## asyncdisposablestack

| feature                                                                                                                            | status       | detail / note       |
| ---------------------------------------------------------------------------------------------------------------------------------- | ------------ | ------------------- |
| test262.test.built-ins.asyncdisposablestack.constructor                                                                            | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.instance-extensible                                                                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.is-a-constructor                                                                       | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.newtarget-prototype-is-not-object                                                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.proto                                                                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.proto-from-ctor-realm                                                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype-from-newtarget                                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype-from-newtarget-abrupt                                                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype-from-newtarget-custom                                                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.adds-value-ondisposeasync                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.not-a-constructor                                                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.prop-desc                                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.puts-value-ondisposeasync-on-top-of-stack                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.returns-value                                                          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.this-does-not-have-internal-asyncdisposablestate-throws                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.this-not-object-throws                                                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.throws-if-disposed                                                     | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.adopt.throws-if-ondisposeasync-not-callable                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.adds-ondisposeasync                                                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.not-a-constructor                                                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.prop-desc                                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.puts-ondisposeasync-on-top-of-stack                                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.returns-undefined                                                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.this-does-not-have-internal-asyncdisposablestate-throws                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.this-not-object-throws                                                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.throws-if-disposed                                                     | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.defer.throws-if-ondisposeasync-not-callable                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.does-not-reinvoke-disposers-if-already-disposed                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.does-not-reinvoke-disposers-if-dispose-already-started          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.does-not-reject-if-already-disposed                             | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.not-a-constructor                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.prop-desc                                                       | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.rejects-with-suppressederror-if-multiple-errors-during-disposal | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.resolves-to-undefined                                           | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.returns-promise                                                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.sets-state-to-disposed                                          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.symbol.asyncdispose-method-not-async                            | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.this-does-not-have-internal-asyncdisposablestate-rejects        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposeasync.this-not-object-rejects                                         | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.does-not-have-asyncdisposablestate-internal-slot                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.getter                                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.name                                                                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.returns-false-when-not-disposed                                     | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.returns-true-when-disposed                                          | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.disposed.this-not-object-throw                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.does-not-dispose-resources                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.not-a-constructor                                                       | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.prop-desc                                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.returns-new-asyncdisposablestack                                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.returns-new-asyncdisposablestack-that-is-still-pending                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.sets-state-to-disposed                                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.still-returns-new-asyncdisposablestack-when-subclassed                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.this-does-not-have-internal-asyncdisposablestate-throws                 | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.this-not-object-throws                                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.move.throws-if-disposed                                                      | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.proto                                                                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.adds-async-disposable-value                                              | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.adds-sync-disposable-value                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.gets-value-symbol.asyncdispose-property-once                             | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.gets-value-symbol.dispose-property-once                                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.not-a-constructor                                                        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.prop-desc                                                                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.puts-value-on-top-of-stack                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.returns-value                                                            | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.symbol.asyncdispose-getter                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.symbol.dispose-getter                                                    | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.this-does-not-have-internal-asyncdisposablestate-throws                  | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.this-not-object-throws                                                   | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-disposed                                                       | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-missing-symbol.asyncdispose-and-symbol.dispose           | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-not-object                                               | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-symbol.asyncdispose-property-is-null-or-undefined        | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-symbol.asyncdispose-property-not-callable                | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-symbol.dispose-property-is-null-or-undefined             | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.prototype.use.throws-if-value-symbol.dispose-property-not-callable                     | 🟢 supported | via rquickjs engine |
| test262.test.built-ins.asyncdisposablestack.undefined-newtarget-throws                                                             | 🟢 supported | via rquickjs engine |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
