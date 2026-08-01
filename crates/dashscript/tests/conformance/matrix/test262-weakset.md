# DashScript Conformance Matrix

- 73 features: **72** supported, **0** partial, **1** unsupported, **0** untested
- correctness cases passing: 0

## weakset

| feature                                                                                                   | status         | detail / note                                              |
| --------------------------------------------------------------------------------------------------------- | -------------- | ---------------------------------------------------------- |
| test262.test.built-ins.weakset.add-not-callable-throws                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.constructor                                                                | 🟢 supported   |                                                            |
| test262.test.built-ins.weakset.empty-iterable                                                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.get-add-method-failure                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.is-a-constructor                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.iterable-failure                                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.iterable-with-object-values                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.iterator-close-after-add-failure                                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.iterator-next-failure                                                      | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.iterator-value-failure                                                     | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.no-iterable                                                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.properties-of-the-weakset-prototype-object                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.proto-from-ctor-realm                                                      | 🔴 unsupported | engine lacks built-in: ReferenceError: $262 is not defined |
| test262.test.built-ins.weakset.prototype-of-weakset                                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.add                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.adds-object-element                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.adds-symbol-element                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.does-not-have-weaksetdata-internal-slot-array                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.does-not-have-weaksetdata-internal-slot-map                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.does-not-have-weaksetdata-internal-slot-object               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.does-not-have-weaksetdata-internal-slot-set                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.does-not-have-weaksetdata-internal-slot-weakset-prototype    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.not-a-constructor                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.returns-this                                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.returns-this-symbol                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.returns-this-when-ignoring-duplicate                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.returns-this-when-ignoring-duplicate-symbol                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-boolean                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-null                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-number                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-string                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-symbol                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.this-not-object-throw-undefined                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.add.throw-when-value-cannot-be-held-weakly                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.constructor.weakset-prototype-constructor-intrinsic              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.delete                                                    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.delete-entry-initial-iterable                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.delete-object-entry                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.delete-symbol-entry                                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.does-not-have-weaksetdata-internal-slot-array             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.does-not-have-weaksetdata-internal-slot-map               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.does-not-have-weaksetdata-internal-slot-object            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.does-not-have-weaksetdata-internal-slot-set               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.does-not-have-weaksetdata-internal-slot-weakset-prototype | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.not-a-constructor                                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.returns-false-when-delete-is-noop                         | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.returns-false-when-value-cannot-be-held-weakly            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-boolean                             | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-null                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-number                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-string                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-symbol                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.delete.this-not-object-throw-undefined                           | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.does-not-have-weaksetdata-internal-slot-array                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.does-not-have-weaksetdata-internal-slot-map                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.does-not-have-weaksetdata-internal-slot-object               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.does-not-have-weaksetdata-internal-slot-set                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.does-not-have-weaksetdata-internal-slot-weakset-prototype    | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.has                                                          | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.not-a-constructor                                            | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.returns-false-when-object-value-not-present                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.returns-false-when-symbol-value-not-present                  | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.returns-false-when-value-cannot-be-held-weakly               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.returns-true-when-object-value-present                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.returns-true-when-symbol-value-present                       | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-boolean                                | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-null                                   | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-number                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-string                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-symbol                                 | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.has.this-not-object-throw-undefined                              | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.prototype.symbol.tostringtag                                               | 🟢 supported   | via rquickjs engine                                        |
| test262.test.built-ins.weakset.undefined-newtarget                                                        | 🟢 supported   | via rquickjs engine                                        |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
