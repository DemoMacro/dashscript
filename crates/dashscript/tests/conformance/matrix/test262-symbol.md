# DashScript Conformance Matrix

- 52 features: **50** supported, **2** partial, **0** unsupported, **0** untested
- correctness cases passing: 0

## symbol

| feature                                                                                                  | status       | detail / note                                                                                        |
| -------------------------------------------------------------------------------------------------------- | ------------ | ---------------------------------------------------------------------------------------------------- |
| test262.test.built-ins.symbol.asyncdispose.cross-realm                                                   | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.asyncdispose.no-key                                                        | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.asynciterator.cross-realm                                                  | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.auto-boxing-non-strict                                                     | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.auto-boxing-strict                                                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.constructor                                                                | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.desc-to-string                                                             | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.desc-to-string-symbol                                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.dispose.cross-realm                                                        | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.dispose.no-key                                                             | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.for.create-value                                                           | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.for.cross-realm                                                            | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.for.description                                                            | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.for.retrieve-value                                                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.hasinstance.cross-realm                                                    | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.isconcatspreadable.cross-realm                                             | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.iterator.cross-realm                                                       | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.keyfor.arg-non-symbol                                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.keyfor.arg-symbol-registry-hit                                             | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.keyfor.arg-symbol-registry-miss                                            | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.keyfor.cross-realm                                                         | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.match.cross-realm                                                          | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.matchall.cross-realm                                                       | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.prototype.description.description-symboldescriptivestring                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.description.get                                                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.description.is-not-own-property                                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.description.this-val-symbol                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.description.wrapper                                              | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.intrinsic                                                        | 🟡 partial   | oracle diff: line 1: ds="{}" node="Object [Symbol] {}" _oracle: diff_                                |
| test262.test.built-ins.symbol.prototype.symbol.toprimitive.redefined-symbol-wrapper-ordinary-toprimitive | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.symbol.toprimitive.this-val-non-obj                              | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.symbol.toprimitive.this-val-obj-non-symbol-wrapper               | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.symbol.toprimitive.this-val-obj-symbol-wrapper                   | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.symbol.toprimitive.this-val-symbol                               | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.tostring.tostring                                                | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.tostring.tostring-default-attributes-non-strict                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.tostring.tostring-default-attributes-strict                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.tostring.undefined                                               | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.valueof.this-val-non-obj                                         | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.valueof.this-val-obj-non-symbol                                  | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.valueof.this-val-obj-symbol                                      | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.prototype.valueof.this-val-symbol                                          | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.replace.cross-realm                                                        | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.search.cross-realm                                                         | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.species.builtin-getter-name                                                | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.species.cross-realm                                                        | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.species.subclassing                                                        | 🟡 partial   | oracle diff: line 1: ds="[Function: MyRegExp]" node="[class MyRegExp extends RegExp]" _oracle: diff_ |
| test262.test.built-ins.symbol.split.cross-realm                                                          | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.toprimitive.cross-realm                                                    | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.tostringtag.cross-realm                                                    | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |
| test262.test.built-ins.symbol.uniqueness                                                                 | 🟢 supported | via rquickjs engine _oracle: matched_                                                                |
| test262.test.built-ins.symbol.unscopables.cross-realm                                                    | 🟢 supported | via rquickjs engine _oracle: node-error_                                                             |

<!-- Generated by `cargo test -p dashscript --test conformance`. Do not edit by hand. -->
