import { useEffect, useMemo, useState } from "react";
import {
  Alert,
  Button,
  Card,
  Empty,
  Flex,
  Input,
  Space,
  Spin,
  Table,
  Tag,
  Typography,
} from "antd";
import { ReloadOutlined } from "@ant-design/icons";
import { api, hex, parseHexInput } from "../api";
import type {
  DeviceInfo,
  HidInterface,
  ProbeResult,
  ProtocolMeta,
  ProtocolTry,
  ReportIdProbe,
} from "../types";
import "./Debug.css";

const { Title, Text } = Typography;

function u16(v: number) {
  return "0x" + v.toString(16).toUpperCase().padStart(4, "0");
}

const QUICK: Array<[string, number[]]> = [
  ["HID++ ping", [0x10, 0xff, 0x10, 0x0f, 0x00, 0x00, 0x05]],
  ["HID++ 1004", [0x11, 0xff, 0x00, 0x0f, 0x10, 0x04, 0x00]],
  ["Nordic 电池", [0x08, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x49]],
  ["CM 查询", [0x55, 0x65, 0x01]],
  ["CM 初始化", [0x55, 0x65, 0x00]],
  ["CM 0xFD 查询", [0xfd, 0x55, 0x65, 0x01]],
  ["CM 0xBC 查询", [0xbc, 0x55, 0x65, 0x01]],
  ["MCHOSE 0x55", [0x00, 0x55, 0x65, 0x01]],
  ["Razer 电量", [0x00, 0x1f, 0x00, 0x00, 0x00, 0x02, 0x07, 0x80]],
  ["AULA AA 10", [0x00, 0xaa, 0x10, 0x00]],
  ["AULA AA A0", [0x00, 0xaa, 0xa0, 0x00]],
  ["AULA 13 05", [0x13, 0x05, 0x01, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x19]],
  ["AULA R55 电量", [0x55, 0x65, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ["AULA R55 EQ", [0x55, 0x73, 0xff, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ["AULA 鼠标 0x30", [0x0b, 0x55, 0x30, 0x01, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ["AULA 鼠标在线 0xED", [0x0b, 0x55, 0xed, 0x01, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
  ["AULA 鼠标信息 0x03", [0x0b, 0x55, 0x03, 0x01, 0x04, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]],
];

export function Debug() {
  const [allDevices, setAllDevices] = useState<DeviceInfo[]>([]);
  const [protocols, setProtocols] = useState<ProtocolMeta[]>([]);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [diagnose, setDiagnose] = useState<ProtocolTry[]>([]);
  const [probeResult, setProbeResult] = useState<ProtocolTry | null>(null);
  const [reportIds, setReportIds] = useState<ReportIdProbe[]>([]);
  const [featureDump, setFeatureDump] = useState<{ ok: boolean; data: string; err?: string } | null>(null);
  const [hexInput, setHexInput] = useState("55 65 01");
  const [sendResult, setSendResult] = useState<ProbeResult | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [protocolId, setProtocolId] = useState<string>("");
  const [msg, setMsg] = useState<string | null>(null);
  const [aulaLog, setAulaLog] = useState<string | null>(null);

  const reload = async () => {
    setLoading(true);
    try {
      const [devs, protos] = await Promise.all([api.enumerateAll(), api.protocols()]);
      setAllDevices(devs);
      setProtocols(protos);
      if (devs.length && !selectedKey) {
        setSelectedKey(devs[0].key);
        setSelectedPath(devs[0].interfaces[0]?.path ?? null);
      }
    } catch (e) {
      setMsg("枚举失败：" + String(e));
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const selected = useMemo(
    () => allDevices.find((d) => d.key === selectedKey) ?? null,
    [allDevices, selectedKey],
  );
  const iface: HidInterface | null = useMemo(() => {
    if (!selected) return null;
    return selected.interfaces.find((i) => i.path === selectedPath) ?? selected.interfaces[0] ?? null;
  }, [selected, selectedPath]);

  const runDiagnose = async () => {
    if (!selectedKey) return;
    setBusy(true);
    setMsg(null);
    try {
      setDiagnose(await api.diagnose(selectedKey));
    } finally {
      setBusy(false);
    }
  };

  const runTry = async (id: string) => {
    if (!selectedKey) return;
    setBusy(true);
    setProtocolId(id);
    try {
      setProbeResult(await api.tryProtocol(selectedKey, id, iface?.path));
    } catch (e) {
      setProbeResult({
        protocol: id,
        name: id,
        ok: false,
        percent: null,
        state: "unknown",
        voltageMv: null,
        detail: String(e),
        raw: [],
        elapsedMs: 0,
      });
    } finally {
      setBusy(false);
    }
  };

  const runCalibrate = async (id: string) => {
    if (!selectedKey) return;
    setBusy(true);
    try {
      const r = await api.calibrate(selectedKey, id, iface?.path);
      setMsg("已标定为「" + r.name + "」，读取到 " + (r.percent ?? "?") + "%");
    } catch (e) {
      setMsg("标定失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const runAulaDiagnose = async () => {
    if (!iface) return;
    setBusy(true);
    try {
      const r: any = await api.aulaDiagnose(iface.path);
      if (r && r.log) {
        const lines = r.log.map(
          (x: any) =>
            x.channel + " cmd=" + x.cmd + " sent=" + x.sent + " -> " +
            (x.replies && x.replies.length ? x.replies.join(" | ") : "(无回复)"),
        );
        setAulaLog(lines.join("\n"));
      } else {
        setAulaLog(String(r?.error ?? "无结果"));
      }
    } catch (e) {
      setAulaLog(String(e));
    } finally {
      setBusy(false);
    }
  };

  const probeIds = async () => {
    if (!iface) return;
    if (!iface.featureLen) {
      setMsg("这个 collection 没有 Feature Report（Feat=0），换一个接口再试");
      return;
    }
    setBusy(true);
    setMsg(null);
    try {
      const list = await api.probeReportIds(iface.path, iface.featureLen);
      setReportIds(list);
      if (list.length === 0) setMsg("4 秒内没有读到可用的 Feature Report");
    } catch (e) {
      setMsg("探测失败：" + String(e));
    } finally {
      setBusy(false);
    }
  };

  const dumpFeature = async (rid: number) => {
    if (!iface) return;
    setBusy(true);
    try {
      const r = await api.readFeature(iface.path, rid, Math.max(iface.featureLen, iface.inputLen, 64), true);
      setFeatureDump({ ok: r.ok, data: hex(r.data), err: r.error ?? undefined });
    } finally {
      setBusy(false);
    }
  };

  const send = async () => {
    if (!iface) return;
    setBusy(true);
    try {
      const bytes = parseHexInput(hexInput);
      const asFeature = iface.featureLen > 0 && bytes.length > iface.outputLen;
      setSendResult(await api.sendProbe(iface.path, hexInput, 600, asFeature));
    } finally {
      setBusy(false);
    }
  };

  const quickSend = async (bytes: number[]) => {
    setHexInput(hex(bytes));
    if (!iface) return;
    setBusy(true);
    try {
      setSendResult(await api.sendProbe(iface.path, hex(bytes), 600, false));
    } finally {
      setBusy(false);
    }
  };

  const firstRid = iface && iface.reportIds.length ? iface.reportIds[0] : 0;
  const nonZeroIds = reportIds.filter((r) => r.nonZero);

  return (
    <Flex vertical style={{ height: "100%", overflow: "hidden" }}>
      <Flex
        align="center"
        justify="space-between"
        style={{ padding: "14px 18px 12px", borderBottom: "1px solid var(--ant-color-border-secondary)", flex: "none" }}
      >
        <div>
          <Title level={5} style={{ margin: 0 }}>HID 调试台</Title>
          <Text type="secondary" style={{ fontSize: 11 }}>
            全部 HID 接口 · Feature Report 转储 · Report ID 探测 · 试发命令 · 手动标定协议
          </Text>
        </div>
        <Button icon={<ReloadOutlined />} onClick={() => void reload()}>重新枚举</Button>
      </Flex>

      <Flex style={{ flex: 1, overflow: "hidden" }}>
        {/* 设备列表 */}
        <div style={{ width: 240, borderRight: "1px solid var(--ant-color-border-secondary)", overflowY: "auto", flex: "none" }}>
          <div style={{ padding: "9px 14px", fontSize: 11, color: "var(--ant-color-text-tertiary)", borderBottom: "1px solid var(--ant-color-border-secondary)" }}>
            设备（{allDevices.length}）
          </div>
          {loading && <Spin style={{ margin: 16 }} />}
          {!loading && allDevices.length === 0 && (
            <Empty image={Empty.PRESENTED_IMAGE_SIMPLE} description="没有 HID 设备" style={{ marginTop: 24 }} />
          )}
          {allDevices.map((d) => (
            <div
              key={d.key}
              onClick={() => {
                setSelectedKey(d.key);
                setSelectedPath(d.interfaces[0]?.path ?? null);
                setDiagnose([]);
                setReportIds([]);
                setFeatureDump(null);
                setSendResult(null);
                setProbeResult(null);
              }}
              style={{
                padding: "8px 14px",
                cursor: "pointer",
                borderBottom: "1px solid var(--ant-color-border-secondary)",
                background: d.key === selectedKey ? "var(--ant-color-primary-bg)" : undefined,
                borderLeft: d.key === selectedKey ? "2px solid var(--ant-color-primary)" : "2px solid transparent",
                color: "var(--ant-color-text)",
              }}
            >
              <div style={{ fontSize: 12.5, fontWeight: 550, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {d.name}
              </div>
              <Text type="secondary" style={{ fontSize: 11 }}>
                {u16(d.vendorId)}:{u16(d.productId)} · {d.kind} · {d.interfaces.length} 接口
              </Text>
            </div>
          ))}
        </div>

        {/* 主区 */}
        <div style={{ flex: 1, overflowY: "auto", padding: "14px 16px 40px" }}>
          {msg && (
            <Alert
              style={{ marginBottom: 10 }}
              type="info"
              showIcon
              closable
              onClose={() => setMsg(null)}
              message={msg}
            />
          )}
          {!selected && <Empty description="左侧选择一台设备" style={{ marginTop: 60 }} />}
          {selected && (
            <Space direction="vertical" size={12} style={{ width: "100%" }}>
              {/* 接口表 */}
              <Card size="small" title="接口列表" extra={<Text type="secondary" style={{ fontSize: 11 }}>{selected.interfaces.length} 个 collection</Text>}>
                <Table
                  size="small"
                  pagination={false}
                  rowKey="path"
                  dataSource={selected.interfaces}
                  scroll={{ x: "max-content" }}
                  rowClassName={(rec) => (rec.path === iface?.path ? "ant-table-row-selected" : "")}
                  columns={[
                    { title: "Usage", dataIndex: "usagePage", render: (_: unknown, i: HidInterface) => <code>{u16(i.usagePage)}:{u16(i.usage)}</code> },
                    { title: "接口", dataIndex: "interfaceNumber", width: 70, render: (v: number) => (v >= 0 ? v : "—") },
                    { title: "In", dataIndex: "inputLen", width: 60 },
                    { title: "Out", dataIndex: "outputLen", width: 60 },
                    { title: "Feat", dataIndex: "featureLen", width: 60 },
                    {
                      title: "Report ID",
                      dataIndex: "reportIds",
                      render: (ids: number[]) => (
                        <span className="mono">
                          {ids.length ? ids.map((x) => x.toString(16).padStart(2, "0")).join(",") : "—"}
                        </span>
                      ),
                    },
                    {
                      title: "",
                      width: 60,
                      render: (_: unknown, i: HidInterface) => (
                        <Typography.Link onClick={() => setSelectedPath(i.path)}>选中</Typography.Link>
                      ),
                    },
                  ]}
                />
                {iface && (
                  <div style={{ marginTop: 10, padding: "7px 10px", background: "var(--ant-color-fill-quaternary)", borderRadius: 8 }}>
                    <Text type="secondary" copyable={{ text: iface.path }} style={{ fontSize: 11, wordBreak: "break-all" }}>
                      {iface.path}
                    </Text>
                  </div>
                )}
              </Card>

              {/* Feature / Report ID */}
              <Card
                size="small"
                title="Feature Report 转储 / Report ID 探测"
                extra={
                  <Space size={6}>
                    <Button size="small" loading={busy} disabled={!iface} onClick={() => void probeIds()}>探测 Report ID</Button>
                    <Button size="small" loading={busy} disabled={!iface} onClick={() => void runAulaDiagnose()}>AULA 握手诊断</Button>
                    <Button size="small" loading={busy} disabled={!iface} onClick={() => void dumpFeature(firstRid)}>
                      转储 0x{firstRid.toString(16).padStart(2, "0").toUpperCase()}
                    </Button>
                  </Space>
                }
              >
                {reportIds.length > 0 && (
                  <Space wrap size={6}>
                    {nonZeroIds.map((rr) => (
                      <Tag key={rr.reportId} style={{ cursor: "pointer" }} onClick={() => void dumpFeature(rr.reportId)}>
                        RID 0x{rr.reportId.toString(16).padStart(2, "0").toUpperCase()}
                      </Tag>
                    ))}
                    {nonZeroIds.length === 0 && <Text type="secondary" style={{ fontSize: 12 }}>没有非全零的 Feature Report</Text>}
                  </Space>
                )}
                {aulaLog && <pre className="hexbox">{aulaLog}</pre>}
                {featureDump && (
                  <pre className={"hexbox" + (featureDump.ok ? "" : " err")}>
                    {featureDump.ok ? featureDump.data || "(空)" : "读取失败：" + featureDump.err}
                  </pre>
                )}
              </Card>

              {/* 试发命令 */}
              <Card size="small" title="试发命令" extra={<Text type="secondary" style={{ fontSize: 11 }}>HEX，空格分隔</Text>}>
                <Space.Compact style={{ width: "100%" }}>
                  <Input
                    value={hexInput}
                    onChange={(e) => setHexInput(e.target.value)}
                    placeholder="55 65 01 00 ..."
                    className="mono"
                  />
                  <Button type="primary" loading={busy} disabled={!iface} onClick={() => void send()}>发送</Button>
                </Space.Compact>
                <Space wrap size={6} style={{ marginTop: 10 }}>
                  <Text type="secondary" style={{ fontSize: 11 }}>常用：</Text>
                  {QUICK.map(([label, bytes]) => (
                    <Tag key={label} style={{ cursor: "pointer" }} onClick={() => void quickSend(bytes)}>{label}</Tag>
                  ))}
                </Space>
                {sendResult && (
                  <div style={{ marginTop: 10 }}>
                    <Text type={sendResult.ok ? "success" : "danger"} style={{ fontSize: 12 }}>
                      {sendResult.ok ? "写入成功" : "写入失败：" + sendResult.error} · {sendResult.elapsedMs}ms
                    </Text>
                    <div><Text type="secondary" style={{ fontSize: 11 }}>发送：{hex(sendResult.sent)}</Text></div>
                    {sendResult.received.length === 0 && <Text type="secondary" style={{ fontSize: 11 }}>无回复</Text>}
                    {sendResult.received.map((rr, i) => <pre key={i} className="hexbox">{hex(rr)}</pre>)}
                  </div>
                )}
              </Card>

              {/* 协议标定 */}
              <Card
                size="small"
                title="协议探测与手动标定"
                extra={<Button type="primary" size="small" loading={busy} onClick={() => void runDiagnose()}>跑遍所有协议</Button>}
              >
                <Space wrap size={6}>
                  {protocols.map((p) => (
                    <Tag.CheckableTag key={p.id} checked={protocolId === p.id} onChange={() => void runTry(p.id)}>
                      {p.name}
                    </Tag.CheckableTag>
                  ))}
                </Space>
                {probeResult && (
                  <Alert
                    style={{ marginTop: 10 }}
                    type={probeResult.ok ? "success" : "error"}
                    showIcon
                    message={
                      probeResult.ok
                        ? `${probeResult.name}：读到 ${probeResult.percent}% · ${probeResult.elapsedMs}ms`
                        : `${probeResult.name}：${probeResult.detail} · ${probeResult.elapsedMs}ms`
                    }
                    action={
                      probeResult.ok ? (
                        <Button size="small" onClick={() => void runCalibrate(probeResult.protocol)}>标定</Button>
                      ) : undefined
                    }
                  />
                )}
                {diagnose.length > 0 && (
                  <Table
                    style={{ marginTop: 12 }}
                    size="small"
                    pagination={false}
                    rowKey="protocol"
                    dataSource={diagnose}
                    rowClassName={(rec) => (rec.ok ? "ant-table-row-selected" : "")}
                    columns={[
                      { title: "协议", dataIndex: "name", width: 160 },
                      { title: "结果", dataIndex: "detail", render: (v: string) => <span className="mono" style={{ fontSize: 11 }}>{v}</span> },
                      { title: "电量", dataIndex: "percent", width: 70, render: (v: number | null) => (v != null ? v + "%" : "—") },
                      { title: "耗时", dataIndex: "elapsedMs", width: 70, render: (v: number) => v + "ms" },
                      {
                        title: "",
                        width: 60,
                        render: (_: unknown, rec: ProtocolTry) =>
                          rec.ok ? <Typography.Link onClick={() => void runCalibrate(rec.protocol)}>标定</Typography.Link> : null,
                      },
                    ]}
                  />
                )}
              </Card>
            </Space>
          )}
        </div>
      </Flex>
    </Flex>
  );
}